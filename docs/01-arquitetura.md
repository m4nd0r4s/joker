# 01 — Arquitetura

Documento normativo. Descreve o desenho final, já com as primitivas M1–M23 que saíram dos documentos 08 e 09 incorporadas. Onde uma secção existe por causa de uma rodada de estresse, a referência aparece entre parênteses.

A semântica do núcleo está no documento 16 e a representação comum na `ref/00`, que prevalecem sobre este documento. As interfaces (manifesto, lock, WIT, drivers, Joker Link, ABI C, API, agente) têm referências exaustivas em `docs/ref/` (ADR 0023); aqui fica a explicação, com remissões.

---

## 1. Visão em camadas

```
┌──────────────────────────────────────────────────────────────────────────┐
│  APPS (componentes Wasm)          Rust · TypeScript · Python             │
│  portas tipadas ─ ciclo (sync) ─ tarefas (async) ─ ações ─ kv            │
├──────────────────────────────────────────────────────────────────────────┤
│  HOST WASM (joker-host)   wasmtime · AOT · pooling · epochs · capacidades │
├───────────────┬───────────────────────────────┬──────────────────────────┤
│  NÚCLEO       │  ML (joker-ml)                │  SERVIÇOS                │
│  (joker-core) │  backends: burn · ort · hailo │  historian (joker-store) │
│  registo      │  · tensorrt                   │  uplink    (joker-link)  │
│  imagem proc. │  modelos .jkm · pipelines     │  API local (joker-api)   │
│  árbitro      │  streams · treino             │  pontes (ROS 2, Zenoh)   │
│  envelope     │                               │  OTA · config · segredos │
│  expressões   │                               │                          │
│  alarmes      │                               │                          │
│  traço        │                               │                          │
├───────────────┴───────────────────────────────┴──────────────────────────┤
│  HAL (joker-hal)   trait Driver · planos de leitura · transportes         │
├──────────────────────────────────────────────────────────────────────────┤
│  DRIVERS  tier N (nativos) · tier W (Wasm) · tier X (processo externo)   │
│  modbus · opcua · gpio · i2c · spi · can/canopen · ethercat · revpi ·    │
│  m-bus · mqtt · serial · v4l2 · sim · …                                  │
├──────────────────────────────────────────────────────────────────────────┤
│  LINUX  PREEMPT_RT (recomendado) · cgroups v2 · systemd · /dev/*          │
└──────────────────────────────────────────────────────────────────────────┘
```

Regras de dependência:

- `joker-core` não depende de nenhuma camada acima nem abaixo. Recebe um `Clock`, recebe amostras e devolve escritas aplicadas. É o que se prova, testa por propriedades e reproduz.
- Os drivers não conhecem apps; as apps não conhecem drivers. O único ponto de encontro é o **caminho de um sinal** resolvido para um **handle denso** (`u32`) no arranque de cada geração de configuração (§15).
- Os serviços (historian, uplink, API) consomem o núcleo apenas através de filas lock-free com limite. Nenhum serviço consegue travar o núcleo (ADR 0013).

---

## 2. Processos e threads

O Joker é **um processo** (`jokerd`) mais processos auxiliares opcionais:

| Processo | Quando existe | Porquê |
|---|---|---|
| `jokerd` | sempre | núcleo, host Wasm, drivers tier N e W, ML em processo, serviços |
| `joker-drv-<nome>` | driver tier X | SDKs de fabricante (blobs em C, licenças), isolamento de falhas |
| `joker-ml-<id>` | modelo com `isolation = "process"` | drivers de GPU/NPU que podem encravar ou rebentar o processo |

Dentro do `jokerd`:

| Thread | Classe | Escalonamento | Função |
|---|---|---|---|
| `rt/<grupo>` (uma por grupo de ciclo) | `rt` | `SCHED_FIFO`, prioridade rate-monotonic (período menor → prioridade maior), fixada a uma CPU `rt` | latch, tarefas de ciclo, árbitro, envelope, commit |
| `rt/supervisor` | `rt` | `SCHED_FIFO` máx. | watchdog dos grupos, expiração de leases, trips, alimenta o watchdog de hardware |
| `rt/drv/<bus>` | `rt` | `SCHED_FIFO` | só para barramentos com ciclo próprio (EtherCAT, piControl com sync) |
| `io` (runtime tokio multi-thread) | `io` | `SCHED_OTHER`, nice −5 | drivers tier N assíncronos, transportes |
| `app` (runtime tokio) | `bg`/declarada | `SCHED_OTHER` | tarefas assíncronas das apps (WASI 0.3) |
| `ml/*` (pool) | `ml` | `SCHED_OTHER` / `SCHED_IDLE` para treino | inferência, pipelines, treino |
| `svc` (runtime tokio) | `bg` | `SCHED_OTHER`, nice 5 | historian, uplink, API, OTA |

**Classes de recursos (M8).** `rt`, `io`, `ml` e `bg` são cpusets (cgroups v2) declarados no manifesto. Numa placa de 4 núcleos, a configuração recomendada é `rt = [3]` com `isolcpus=3 nohz_full=3 rcu_nocbs=3` no kernel, `io = [2]`, `ml = [1,2]` e `bg = [0]`. O `jk doctor` verifica isto (documento 07).

**Porque é um processo só.** A EdgeX paga latência e memória por cada microserviço, e o Kura paga a JVM. Com um processo, uma leitura vai do driver à app sem serialização nem IPC: uma cópia de 32 bytes para o slot, outra para o snapshot. O isolamento de falhas, que é o argumento a favor de vários processos, obtém-se de outra forma. As apps estão em sandbox Wasm (um trap não derruba o processo), e o código em que não se confia com memória nativa (blobs de fabricante, drivers de GPU) vai para processos tier X.

---

## 3. O núcleo: registo de sinais e imagem de processo

### 3.1 Registo

No arranque de cada geração de configuração, o manifesto e o `joker.lock` são compilados para um **registo imutável**:

- cada sinal tem um `SignalId(u32)` denso, ordenado por caminho (o que dá determinismo);
- metadados: tipo, unidade canónica, direção, dispositivo dono, `max_age`, envelope (se for saída), grupo dono (se for saída), política de gravação;
- tabelas derivadas: sinais por driver, portas por app, dependências de expressões.

**Nenhum lookup por string acontece no caminho quente.** As apps recebem handles na inicialização; as expressões são compiladas para bytecode sobre `SignalId`.

### 3.2 Armazém de últimos valores

Cada sinal escalar tem um **slot de 32 bytes** alinhado a cache line (dois por linha):

```
struct Slot {            // 32 bytes
    version: AtomicU32,  // seqlock: ímpar = escrita em curso; seq da amostra = version / 2
    quality: u32,        // código (8 bits) + motivo (8 bits) + flags (16 bits, inclui o domínio de ts_src)
    value:   u64,        // bits do valor (bool/i64/u64/f64/f32 promovido; enum = u64)
    ts_src:  i64,        // ns: UTC do dispositivo remoto, ou monotónico de hardware local (flag); i64::MIN se não houver
    ts_acq:  u64,        // ns monotónicos (CLOCK_MONOTONIC) da receção
}
```

**Domínio do carimbo.** O campo `ts_src` guarda um de três carimbos, e as flags da palavra de qualidade dizem qual: o UTC do dispositivo remoto (`ts_src` propriamente dito), o monotónico de captura em hardware local (`ts_hw`), ou o `ts_hw` de um MCU **convertido** para o monotónico deste nó pela estimativa do `TIME` do Joker Link (domínio "link", `ref/07` §8.3). Um carimbo convertido tem a incerteza da estimativa, e por isso nunca se confunde com um local. A disposição exata dos bits está na `ref/00` §4.3 e na `ref/04` §3.2.

Cada slot tem **um único escritor**: o estágio de aquisição do driver dono (ingestão) ou, para sinais derivados, o grupo de ciclo dono. Os leitores (latch dos grupos, tarefas assíncronas, historian) usam o protocolo seqlock, com uma regra: **um leitor nunca espera por um escritor**. Se uma leitura colide com uma escrita em curso, o leitor repete-a um número limitado de vezes e, se continuar a colidir, usa a amostra completa anterior e conta a colisão. Num MCU de um núcleo, onde o leitor pode ser uma interrupção que preemptou o próprio escritor, não repete: usa logo a anterior (`ref/08` §2.4.1). Um leitor que esperasse ficaria bloqueado para sempre. Strings e `bytes` curtos (≤ 64 B) vão para uma arena à parte com double buffering. Os maiores são streams (§11).

A `seq` de 64 bits exposta às apps é a versão do seqlock a dividir por 2, estendida pelo leitor com um contador de voltas por sinal guardado fora do slot (uma volta a cada 2³¹ amostras). Custo alvo: escrita ~20 ns, leitura ~10 ns num Cortex-A76.

### 3.3 Aquisição

```
driver ──RawSample──▶ fila SPSC do driver ──▶ estágio de aquisição ──▶ slot
                                                 │ escala/offset/byte order
                                                 │ conversão de unidade
                                                 │ qualidade: link, limites físicos, NaN
                                                 │ deadband de aquisição
                                                 └▶ evento "changed" (bitset por grupo)
```

O estágio de aquisição corre na thread `io`, logo depois do driver, e é código do núcleo: os drivers entregam valores crus mais o estado do link, e a semântica (escala, unidade, qualidade) é aplicada num único sítio.

**Envelhecimento.** Cada sinal tem `max_age` (por omissão, 3 × o período de poll). No latch, uma amostra mais velha do que `max_age` é marcada `Uncertain(Stale)`, e mais velha do que `2 × max_age` passa a `Bad(Stale)`. Não se muda o slot: calcula-se no momento da leitura, porque a idade depende de quando se lê.

### 3.4 Imagem de processo por grupo

Cada grupo de ciclo tem uma **imagem de processo própria**: um vetor denso só com os sinais que as suas tarefas leem (as entradas ligadas) e escrevem (as saídas donas). É pré-alocado no arranque.

- **Latch**: no início do tick, o grupo copia os slots relevantes para o seu snapshot. Todas as tarefas do tick veem exatamente os mesmos valores.
- **Staging**: as escritas das tarefas vão para um buffer de propostas do grupo, e não para os slots.
- **Commit**: no fim do tick, as propostas passam pelo árbitro e pelo envelope (§6, §7). As aplicadas vão para os slots de saída e para as filas de saída dos drivers.

**Caixas de saída para os drivers.** O commit não escreve numa FIFO. Cada saída tem uma **caixa latest-wins** (o último valor aplicado substitui o anterior ainda não enviado), de modo que um equipamento lento ou encravado nunca recebe, ao acordar, uma fila de valores velhos. Só as escritas agendadas, os impulsos e os grupos atómicos usam uma fila ordenada, e essa é limitada. Cada caixa tem ainda um bit atómico `force_safe` que só o supervisor ativa (documento 09, ataque 4): é o caminho de emergência quando um grupo encrava, e não viola o escritor único dos valores.

Consistência entre grupos: um grupo lê os valores **aplicados** (commits) de outro grupo, no estado em que estavam no seu latch. Não há consistência transacional entre grupos de períodos diferentes, e isso é declarado (documento 02, §6.3). Quem precisa de consistência entre duas tarefas põe-nas no mesmo grupo.

---

## 4. O tick de um grupo de ciclo

```
 t_k = t_0 + k·T  (clock_nanosleep TIMER_ABSTIME, CLOCK_MONOTONIC)
 │
 ├─ 1. latch               copia N_in slots → snapshot            (~10 ns/sinal)
 ├─ 2. pedidos async       drena inbox de propostas async → staging
 ├─ 3. expressões          derivados, alarmes e permissivos do grupo (bytecode)
 ├─ 4. tarefas             para cada tarefa, pela ordem declarada (`order` do grupo):
 │                           chamada Wasm síncrona com deadline por epoch
 │    4b. comandos virtuais  se uma tarefa posterior implementa uma instância virtual, os comandos
 │                           desta tarefa são arbitrados e passam pelo envelope já aqui (documento 17, §5.3)
 ├─ 5a. métodos            expandir os registos de método das propostas, pela ordem das chamadas (documento 17, §4.1)
 ├─ 5. árbitro             por saída: escolhe o lease ativo de maior prioridade (regras: documento 16, §1)
 ├─ 6. envelope            por saída: permissivo → gama → taxa → deadband → grupos atómicos
 ├─ 7. commit              slots de saída + caixas latest-wins dos drivers + escritas agendadas
 ├─ 8. traço               entrada no anel: seq do tick, seqs lidas, resultados das escritas
 └─ 9. estatísticas        jitter, duração, overruns (histogramas sem alocação)
```

**Orçamento.** Cada tarefa declara ou mede (`jk bench`) o seu WCET. O `jk check` soma os orçamentos por CPU `rt` e aplica o teste de escalonabilidade rate-monotonic (Liu & Layland, com margem de 30%, e a análise de tempo de resposta quando o limite não chega; a fórmula e o que entra no WCET estão na ADR 0048). Um manifesto que não passa é recusado (**admissão de tempo real**, M23). É o que impede alguém de pôr um grupo de 1 ms com 900 µs de tarefas e descobrir o problema em campo. **A admissão não garante ausência de overruns num núcleo partilhado:** o WCET é medido com a tarefa sozinha (`jk bench`), e com a CPU `rt` partilhada com `io`, `bg` ou o sistema (uma placa com `cpus = 1`, ou sem `isolcpus`), as interrupções, a cache e o escalonador de outras classes somam-se ao tempo real; a margem de 30% amortece, não prova, e os overruns contam-se sempre (`jk status`).

**Deadline e overrun.** Cada chamada Wasm tem um deadline por epoch: o supervisor incrementa a epoch do motor a cada 100 µs, e a instância é interrompida se ultrapassar `max(wcet × 2, 200 µs)` (a granularidade da epoch não permite deadlines mais finos; o WCET de tarefas curtas é vigiado por medição, não por interrupção). Se o tick inteiro ultrapassar o período, aplica-se a política do grupo:

| `overrun` | Comportamento |
|---|---|
| `skip` (omissão) | o tick seguinte é saltado para realinhar; conta `overruns` |
| `catch_up` | corre imediatamente, até 1 vez; só para grupos sem efeitos dependentes do tempo |
| `degrade` | desativa as tarefas marcadas `optional` até N ticks limpos |
| `trip` | põe todas as saídas do grupo em estado seguro |

N overruns consecutivos (por omissão 10) geram o alarme `joker.group.<g>.overrun` e, se `trip_after` estiver declarado, o trip do grupo.

**Tarefas nativas.** Para os raros casos em que a fronteira Wasm pesa (1 kHz com muita aritmética), uma tarefa de ciclo pode ser escrita como crate Rust compilado dentro do `jokerd` (`kind = "native"`). Perde-se o sandbox e o hot-swap e ganham-se 1–3 µs por chamada. A regra é Wasm por omissão e nativo com justificação.

---

## 5. O domínio assíncrono

As tarefas assíncronas (`#[joker::task]`) correm na runtime tokio `app` como exports `async` do WASI 0.3. Servem para eventos, temporizadores lentos, ML, cálculo pesado, rede autorizada e ações.

- **Leitura**: veem o *último valor* (slot), não um snapshot, e cada leitura traz `seq` e `ts`. Para ler vários sinais de forma consistente, pedem `snapshot([h1, h2, …])`, que repete a leitura seqlock até obter uma época coerente.
- **Escrita**: é sempre uma proposta que vai para a inbox do **grupo dono** da saída (SPSC por par tarefa→grupo). É aplicada no próximo tick desse grupo e devolve `Deferred`, com o resultado final a chegar por um future (`WriteTicket`).
- **Nunca tocam no domínio de ciclo**: não há chamada síncrona do domínio assíncrono para o de ciclo, nem ao contrário.

**Uma app, duas instâncias.** Um `Store` do wasmtime não pode ser usado por duas threads ao mesmo tempo. Uma app com tarefas de ciclo e tarefas assíncronas é portanto instanciada **duas vezes** a partir do mesmo componente: uma instância de ciclo (thread `rt/<grupo>`) e uma assíncrona (tokio `app`). Não partilham memória linear. Comunicam por **sinais internos** da app (`#[internal]`, que vivem em slots como os outros) e por **caixas de correio** limitadas. O SDK torna isto explícito no sistema de tipos (documento 04, §3.4). A decisão e as alternativas rejeitadas estão no documento 09, ataque 6.

---

## 6. O árbitro (M15)

Cada saída tem um árbitro com **16 níveis de prioridade**, em que 1 é o mais alto (como o *priority array* do BACnet). Cada escritor autorizado ocupa um nível, declarado na ligação:

| Faixa | Nome | Uso típico |
|---|---|---|
| 1–2 | `safety` | apps de proteção (software, nunca substituem proteção de hardware) |
| 3–5 | `manual` | operador (CLI, API, HMI) |
| 6–8 | `supervisor` | coordenador do site, redução por ordem do operador de rede |
| 9–12 | `control` | controladores de regulação |
| 13–15 | `optimizer` | otimizadores, modelos de ML |
| 16 | `schedule` | calendários, valores de base |

Semântica:

1. Uma escrita de um escritor coloca `(valor, lease_até)` no seu nível. Cada escrita renova o lease.
2. O valor efetivo é o do **nível ativo de maior prioridade** (lease não expirado).
3. `relinquish()` liberta o nível de imediato.
4. **Sem nenhum nível ativo, a saída vai para o estado seguro.** Não há valor de *relinquish default* configurável separado do `safe`: um valor "de repouso" que não fosse seguro seria uma segunda verdade.
5. Os escritores `manual` têm um **TTL obrigatório em cada escrita**, sem omissão; o máximo é o `max_manual` da saída (omissão 8 h). O override esquecido é o problema mais comum em sistemas de gestão de edifícios, e aqui expira sozinho. 15 minutos antes do fim, o alarme `joker.output.<s>.manual_expiring` (gravidade `medium`) e um evento no tópico `manual`; com um TTL ≤ 15 min, só o evento, porque um alarme imediato seria ruído (`ref/09` §5.6). Os níveis manuais sobrevivem a um reinício do `jokerd`, com o prazo em UTC (16, A14); se o relógio não estiver `synced` no arranque, repõe-se o TTL restante e levanta-se o alarme `joker.output.<s>.manual_restored` (gravidade `medium`), para que um operador confirme ou liberte.
6. Cada nível só aceita o escritor declarado. Duas apps não partilham um nível, e duas ligações de apps diferentes na mesma saída sem níveis distintos são um erro de validação (`J0210`). Num nível manual, ocupado em runtime por uma pessoa, um segundo principal é recusado a não ser que peça `takeover` (16, A13).
7. Escritores só-proposta (M18, **1.1**) não ocupam nível: criam **intenções** que só passam a escrita `manual` depois de aprovação humana (§6.1).

Numa saída de um MCU, o árbitro é o do MCU; o nó Linux é um escritor remoto em níveis delegados (16, A12).

O árbitro corre **antes** do envelope. O envelope aplica-se a quem ganhou, seja quem for, incluindo `safety` e `manual`.

### 6.1 Intenções e aprovação (M18, **1.1**)

Reservado para a 1.1 (D5): `writer.kind = "proposal"` é `J0290` numa runtime 1.0, e os endpoints de intenções respondem vazio. O desenho fica aqui. Um escritor com `writer.kind = "proposal"` (um assistente LLM, um otimizador em teste, um sistema externo sem confiança) não escreve: chama `propose(saída, valor, justificação, ttl)`. A intenção fica pendente na API local, e um humano com o papel `operator` aprova-a ou rejeita-a (`jk intent approve <id>`, ou uma HMI). Aprovada, é executada como escrita `manual` com o TTL da intenção, em nome de quem aprovou. Tudo fica no registo de decisões.

---

## 7. O envelope

Aplicado por saída, no commit do grupo dono, pela ordem seguinte. O primeiro passo que falha decide o resultado. As regras exatas estão no documento 16, §2 (E1–E10).

| # | Passo | Falha → |
|---|---|---|
| 0 | **relógio**: o escritor vencedor é uma app com `requires_clock = "synced"` e o relógio do nó não está `synced` (§18) | `Rejected(ClockUnsynced)`, e a saída vai para `safe`; renova o lease (16, A3 e E1) |
| 1 | tipo e unidade (já convertida pela ligação) | `Rejected(WrongType)`; nunca acontece se o `jk check` passou |
| 2 | **permissivo** (`interlock`): expressão booleana sobre sinais; `Bad` conta como falso | `Rejected(Interlock)`, e a saída mantém o valor anterior ou vai para `safe`, conforme `on_interlock` |
| 3 | **qualidade das dependências** (`requires_good`): sinais que têm de estar `Good` | `Rejected(Quality)` depois de `grace`, e a saída mantém o valor anterior ou vai para `safe`, conforme `on_interlock` (16, E4) |
| 4 | **gama** (`range`) | `clamp` → `Clamped(Range)`; `reject` → `Rejected(Range)`; `trip` → `Tripped` |
| 5 | **taxa** (`rate`, unidade/s) em relação ao último valor aplicado | `Clamped(Rate)` (aplica o passo máximo) |
| 6 | **deadband**, **intervalo mínimo** (`min_interval`) e **orçamento de atuações** (`max_actuations = "20/h"`, para relés e contactores) | `Suppressed`: não é erro, e não gasta o barramento; o orçamento esgotado dá `Rejected(Budget)` e alarme |
| 7 | **grupos atómicos** (M20): se a saída pertence a um grupo atómico, todas as do grupo passam juntas ou nenhuma passa | `Rejected(AtomicGroup)` |
| 8 | commit | `Accepted` |

O `safe` é um valor ou `relinquish`, que devolve o controlo à lógica do próprio equipamento ou ao sistema anterior (documento 03, §6.1). **O valor seguro é sempre admissível.** Ir para `safe` não é bloqueado por permissivos nem por gama. A taxa só se aplica se houver `safe_ramp` declarado, porque há processos em que saltar para o seguro é perigoso: fechar uma válvula de repente provoca golpe de aríete.

**Trip.** Uma saída em trip fica em `safe` até haver um `reset` explícito (ação `manual` ou condição `reset_when`). O trip é persistente entre reinícios do `jokerd`, porque fica em `state/trips`, com fsync.

**Verificação por leitura de retorno.** Uma saída pode declarar `verify = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }`. Se o valor lido não confirmar o escrito, a saída passa a `Uncertain(NotConfirmed)` e o alarme `joker.output.<s>.unconfirmed` dispara. É o que apanha o caso "o equipamento aceitou o Modbus e ignorou o valor".

---

## 8. Estado seguro em três camadas

| Camada | Quem a garante | O que cobre | O que não cobre |
|---|---|---|---|
| **Lease da app** | árbitro + supervisor `rt` | app morta, encravada, modelo lento, rede de uma tarefa async | `jokerd` morto |
| **Supervisor do jokerd** | thread `rt/supervisor` + watchdog de hardware (`/dev/watchdog`, RevPi, IPC) + `WatchdogSec` do systemd | grupo encravado, deadlock, runtime morta → reinício da placa | comunicação com o equipamento perdida |
| **Fallback do dispositivo (M1)** | o próprio equipamento | tudo o que está acima, incluindo o cabo cortado | nada: é a última linha |

**M1: o fallback do dispositivo é declarado e verificado.** Os perfis declaram como o equipamento se protege (registo de timeout de comunicação, valor de fallback). No arranque, o driver lê esses registos e, se o manifesto o permitir, escreve-os. Depois compara-os com o `safe` do Joker. As regras:

- `device_fallback.value` tem de ser **tão seguro ou mais** do que o `safe` do Joker (o sentido de "mais seguro" é declarado por saída: `safer = "lower" | "higher" | "equal"`);
- a runtime **renova a escrita** no equipamento a cada `keepalive` (por omissão, 1/5 do timeout do equipamento), mesmo que o valor não mude; o deadband e o `min_interval` nunca suprimem um keepalive;
- `device_fallback.timeout` tem de ser **maior** do que `keepalive` mais o tempo de reinício do `jokerd` (medido pelo `jk doctor`), senão o equipamento cai para fallback a cada atualização da runtime. Os leases dos escritores não entram nesta conta: são internos ao Joker e, quando expiram, a runtime escreve o `safe` ativamente;
- um reinício da **placa** inteira (watchdog de hardware) demora mais do que o timeout, e o fallback do equipamento é o comportamento certo nesse caso;
- um fallback em falta ou incoerente é `warning` em `dev` e **erro** em `prod`, a não ser que haja `accept_missing_fallback = "<justificação>"`, que fica no `joker.lock`.

---

## 9. Host Wasm (joker-host)

- **Motor:** wasmtime (≥ 46, WASI 0.3 com async nativo). Cranelift em x86_64, aarch64 e riscv64; o interpretador Pulley em armv7 e noutras arquiteturas sem Cranelift (documento 08, rodada 5).
- **AOT obrigatório em produção.** Os componentes são pré-compilados (`.cwasm`) no `jk build --target` ou na instalação no dispositivo, com o hash do motor e das flags de CPU. Nunca há JIT no caminho do ciclo.
- **Imagem empacotada.** O caminho rápido do ciclo é uma única chamada por tarefa: `on-cycle(task, image) -> cycle-result`. A imagem de entradas da app vai empacotada (24 B por amostra e 24 B por resultado de saída) e as propostas voltam num buffer de registos empacotados (uma escrita são 14 B: tipo, flags, porta `u32`, valor `u64`). O layout exato está na `ref/04` §3. Os SDKs de tier 2 (TypeScript, Python) usam a **mesma** imagem e o mesmo buffer; as chamadas `cycle-io.read/write` ficam só para acessos dinâmicos e para valores que não cabem em `u64` (documento 09, ataque 2; `ref/05`).
- **Tier 2 no ciclo** (D15): permitido, com período mínimo de 100 ms em TypeScript e 500 ms em Python; a admissão usa o **máximo** medido pelo `jk bench`, e não o p99,9, porque as pausas de GC tornam o percentil enganador.
- **Determinismo do motor:** relaxed SIMD desativado e NaN canonicalizados nas instâncias de ciclo, para que o mesmo `.wasm` dê os mesmos bits em aarch64 e x86_64 (documento 09, ataque 10).
- **32 bits:** em armv7, memórias com verificação explícita de limites e sem as reservas virtuais de 4 GiB (documento 09, ataque 3).
- **Pooling allocator.** Memória linear e tabelas pré-reservadas por instância, com `memory.max` declarado pela app. A omissão depende do tier: 16 MiB no ciclo e 64 MiB no async em Rust; 64/64 MiB em TypeScript; 128/128 MiB em Python (o SDK põe a sugestão no `joker:meta`, e o manifesto vence: `ref/01` §5.12, `ref/05` §19). As páginas da instância de ciclo são pré-tocadas e fixadas. Alocar no ciclo, fora do que foi reservado no `init`, é trap (`J-E0903`, `ref/04`).
- **Deadlines:** epoch interruption (§4). O fuel é opcional e só se usa em testes deterministas.
- **Capacidades.** Uma app só recebe handles das portas ligadas. Por omissão não tem sistema de ficheiros, sockets nem relógio de parede (usa `cx.now()`, que vem do tick). Rede para tarefas async só com `net.allow = ["api.example.com:443"]`, e ficheiros só em `kv` (§9.1).
- **Falhas.** Uma tarefa que termina com erro (`cycle-result.error`) mantém as propostas que já pôs no buffer. Um trap numa tarefa de ciclo conta como falha do tick dessa app: as suas propostas do tick são descartadas e os leases continuam a correr. K traps em janela (por omissão 3 em 10 s) põem a app em `crashloop`, com a instância descartada, recriação com backoff exponencial (1 s → 5 min) e alarme. As saídas seguem o lease e vão para o seguro sozinhas.
- **Hot-swap.** Uma nova versão é instanciada ao lado, recebe o estado via `migrate(old_state) -> new_state` (opcional) e entra num **limite de tick**: no tick k corre a versão antiga, no k+1 a nova. Se o `init` falhar, a antiga continua.

### 9.1 Estado persistente das apps

`kv`: armazenamento chave-valor pequeno (≤ 1 MiB por app), transacional, com fsync configurável. Só as tarefas async lhe têm acesso. O ciclo não faz I/O: se precisa de estado persistente, publica um sinal interno e uma tarefa async guarda-o.

---

## 10. ML (v1.1; resumo; detalhe no documento 06)

O ML não faz parte da v1 (ADR 0019). O que a v1 já reserva para ele está no documento 11, "Preparado para ML".

- Um **modelo** é declarado no manifesto como um dispositivo: artefacto `.jkm`, assinatura de tensores, backend com cadeia de fallback, deadline, classe de recursos e isolamento.
- As apps chamam `infer` (async) com tensores pequenos, ou consomem as saídas de **pipelines nativos** (M5), que levam streams diretamente ao acelerador sem passar pela memória Wasm.
- Os backends implementam o trait `InferenceBackend`. O **Burn** é o de omissão e de referência (CPU Flex/CubeCL, Vulkan, CUDA, Metal para desenvolvimento). ORT, HailoRT e TensorRT são plugins.
- **Guarda de entrada (M17):** a assinatura declara gamas plausíveis por entrada, e uma entrada fora da distribuição devolve `Uncertain(OutOfDistribution)` junto com o resultado, que a app ou o controlador de fallback tratam.
- **Saídas não deterministas (M22):** as saídas de modelos em GPU/NPU são gravadas no traço, e o replay usa-as em vez de reexecutar.

---

## 11. Streams e dados volumosos (M4)

Imagens, nuvens de pontos e blocos de áudio não cabem em slots de 32 bytes. Um **stream** é:

- um **pool de buffers** pré-alocado (memfd ou dmabuf quando o driver o suporta: V4L2, libcamera), de tamanho fixo por stream;
- **frames com contagem de referências**, com metadados (ts de captura em hardware quando existe, seq, formato, dimensões);
- semântica `latest` (os consumidores lentos perdem frames, e isso é contado) ou `queue(n)` (com contrapressão sobre o produtor até n e perda a partir daí).

Um frame nunca é copiado entre produtor e consumidores nativos. As apps Wasm recebem um **recurso** `frame` e podem pedir `view(roi, formato)`, que copia só a região pedida para a memória linear. O caminho normal para ML é o pipeline nativo (documento 06, §5).

**Sinais em bloco (M7).** Para formas de onda (vibração a 25,6 kS/s, corrente a 10 kS/s), o driver entrega **blocos**: `t0`, `período`, `n`, amostras contíguas e qualidade por bloco. Vivem num anel por sinal, e os consumidores leem blocos inteiros. O historian guarda-os comprimidos, e os derivados escalares (RMS, pico, bandas FFT) são calculados por pipelines ou tarefas.

---

## 12. Historian local (joker-store)

- **Segmentos por série**, append-only, com blocos de 4 KiB: compressão Gorilla (delta-of-delta nos tempos, XOR nos floats) para escalares, e zstd para blocos de forma de onda e eventos.
- **WAL** partilhado, com fsync em lote (por omissão a cada 1 s, `fsync = "batch" | "always" | "never"`). Perde-se no máximo 1 s numa falta de energia. Usam `always` só os trips, as escritas manuais, as rejeições e as mudanças de nível ativo; o resto do registo de decisões vai em lote (§19).
- **Retenção por camadas:** `raw` (p. ex. 7 d), agregados `1s`, `1m` e `15m` (min/max/média/última/contagem/qualidade dominante), cada um com o seu `keep`. A retenção por tamanho tem prioridade sobre a retenção por tempo (`max_disk = "8 GiB"`). Com o disco a 90%, apaga-se primeiro o `raw` mais antigo.
- **Consulta:** API local (`/v1/history`), `jk history`, exportação em Parquet. O Parquet é escrito pela `jk`, a partir do NDJSON da API: o nó não leva um escritor de Parquet (`ref/09` §5.5).
- **Store-and-forward:** cada uplink tem um cursor persistente sobre o historian, e o reenvio lê do historian em vez de manter uma fila separada. Uma queda de rede de dias não custa memória, só disco, e está limitada pela retenção.

Alvo: 200 k amostras/s de ingestão num CM4 com eMMC, com 1–2 bytes por amostra comprimida em sinais de processo típicos.

**O meio de armazenamento conta.** A placa declara `storage = "sd" | "emmc" | "ssd"` (`ref/03` §2.3.1). O `jk check` projeta a carga de escrita do manifesto (historian, registo de decisões, `kv`, traço contínuo, e o número de fsyncs) e o `jk doctor` mede-a no nó; acima do que o meio aguenta, o aviso `J0381` diz quanto e de onde vem. Um cartão SD é o caso mais frágil, e é o da bancada (documento 23).

---

## 13. Uplink (joker-link)

| Tipo | Uso |
|---|---|
| `mqtt` | genérico, tópicos e JSON/CBOR configuráveis |
| `mqtt-sparkplug` | Sparkplug B: NBIRTH/DBIRTH, métricas com alias, estado do nó; o mapeamento de sinais para métricas é automático |
| `zenoh` | pub/sub/query eficiente, interop com ROS 2 (`rmw_zenoh`), malhas locais |
| `otlp` | métricas, logs e traços **da própria runtime** para observabilidade |
| `http` | webhook de eventos e alarmes |

**Orçamentos (M12).** Cada uplink pode declarar `budget = "100 MB/month"` e classes de prioridade (`alarms > events > telemetry.high > telemetry.low > bulk`). Um estimador de consumo projeta o mês. Quando a projeção ultrapassa o orçamento, as classes baixas passam a agregados (1 s → 1 min → 15 min) antes de alguma coisa ser descartada. Os alarmes nunca são degradados. O estado do orçamento é um sinal (`joker.link.<u>.budget_used`).

**Entrega:** `at_least_once` com número de sequência por série (o destino deduplica) para histórico, e `latest` para valores ao vivo. A ordem é garantida por série, não entre séries.

---

## 14. API local e pontes

- **API local** (`joker-api`): HTTP/1.1 e HTTP/2 com JSON, WebSocket e SSE para subscrições e OpenAPI publicada. Escuta num socket Unix (`/run/joker/api.sock`) por omissão e, opcionalmente, em TCP, sempre com mTLS. Papéis: `viewer`, `operator` (escritas manuais, aprovação de intenções, reset de trips), `admin` (config, OTA). A CLI `jk` é apenas um cliente desta API. A referência exaustiva (endpoints, erros, anti-replay, WebSocket) está na `ref/09` e no `spec/openapi/joker-api.yaml`.
- **Pontes (M11):** exposição bidirecional de sinais para outros ecossistemas: ROS 2 via Zenoh (`rmw_zenoh`), OPC UA **servidor** (o Joker publica o seu espaço de endereços para SCADA), Modbus **servidor** (para PLCs legados que querem ler do Joker). **Uma escrita que chega por uma ponte é uma escrita de um escritor declarado**, com prioridade, lease e envelope. Uma ponte não é um atalho.

---

## 15. Configuração: gerações e aplicação transacional

Uma **geração** é o par (manifesto resolvido, `joker.lock`) com o registo compilado. Mudar de configuração é mudar de geração. Cada geração tem um número `gen`, dado na criação, e um hash do conteúdo, que a identifica (o do descritor `joker.config.v1+cbor`, `ref/10` §2.6.2).

```
jk apply  ──▶ validar (J-codes) ──▶ planear diff ──▶ preparar ──▶ trocar ──▶ janela de confirmação ──▶ confirmado
                                         │                            │                │
                                         │                            │                └─ expirou → rollback automático
                                         │                            └─ num limite de tick de cada grupo afetado
                                         └─ o que muda: drivers a reiniciar, apps a recarregar,
                                            envelopes a trocar (a quente), sinais novos/removidos
```

- **Diff mínimo:** mudar um envelope não reinicia o driver, e mudar o endereço de um dispositivo reinicia só esse driver. Os sinais que sobrevivem mantêm o `SignalId` quando possível, e as apps não afetadas continuam sem interrupção.
- **Confirmação (ADR 0015):** `jk apply --confirm 5m` aplica e espera um `jk confirm`. Se ele não chegar (porque a nova config cortou a rede, por exemplo), volta sozinho à geração anterior. É o `commit confirmed` dos routers aplicado ao edge. Em `prod`, a janela é **obrigatória** quando o plano muda a rede, a segurança, os uplinks ou a **própria API** (pontos de escuta, papéis): são os `confirm_reasons` do plano (`network`, `security`, `uplinks`, `api`, `ref/09` §5.13).
- **Rollback reativa** (D9): voltar atrás não cria uma geração nova. A geração anterior volta a ser a ativa com o mesmo hash e o mesmo `gen`, e o histórico de gerações é um histórico de **ativações**, cada uma com a sua origem (`origin`; `ref/09` §5.13, `ref/10`). Uma geração ativada que não chega a ser confirmada fica `rolled_back`; uma `pending` que nunca foi ativada fica `discarded` (`ref/00` §6, `ref/09` §5.13).
- **Sobreposições (M13):** `extends = ["templates/joker.bess-v2.toml"]` com fusão profunda e regras explícitas (documento 03, §2). O `jk diff` mostra o manifesto efetivo.

---

## 16. OTA: quatro tipos de artefacto

| Artefacto | Unidade | Troca | Rollback |
|---|---|---|---|
| **runtime** (`jokerd`) | binário assinado | slots A/B em `/var/lib/joker/runtime/{a,b}`; o systemd arranca o ativo; teste de saúde (N ticks limpos, drivers ligados) | automático se a saúde falhar dentro de `health_window` |
| **app** | componente `.wasm` assinado + `.cwasm` | hot-swap num limite de tick (§9) | versão anterior mantida quente durante `keep_previous` |
| **modelo** | `.jkm` assinado | modo sombra → promoção (M16) | despromoção automática por KPI |
| **config** | manifesto + lock | geração com confirmação (§15) | janela de confirmação |

Uma runtime nova arranca **com a geração confirmada atual**: o OTA da runtime nunca muda de configuração ao mesmo tempo (`ref/10` §3). Durante o reinício, os equipamentos cobrem o intervalo com o seu fallback (M1) e os MCUs com os seus leases.

O Joker **não atualiza o SO**. Integra-se com RAUC, SWUpdate ou Mender para isso (documento 12). A gestão da frota é do **Joker Deck** (documento 20, ADR 0021): o `jokerd` expõe um agente com estado desejado e estado reportado (ref/10), que o Deck, ou outro gestor, usa.

---

## 17. Segurança

- **Identidade do dispositivo:** certificado X.509 **P-256** emitido pela CA da organização, com a chave em TPM 2.0 ou elemento seguro quando existe, e em ficheiro protegido quando não existe (fica declarado no `jk doctor`). O enrolamento cria também a chave X25519 do nó para os segredos (ADR 0025, `ref/02` §2.5).
- **Assinaturas:** runtime, apps, drivers, perfis, placas, classes, firmware, configs de produção e segredos são assinados com chaves **Ed25519** da organização, fora dos nós; os documentos assinados vão em COSE_Sign1 sobre CBOR (ADR 0025). A política de confiança (que chave assina que papel) vem de um **bundle** assinado por uma raiz fora de linha; o `[security.trust]` do manifesto só a restringe. O `jokerd` recusa artefactos não assinados em `mode = "prod"`.
- **Memória:** `oom_score_adj = -900` no `jokerd` e `+500` nos processos `joker-ml-*`/`joker-drv-*`, para que o kernel mate primeiro o modelo e não o controlo.
- **Privilégios mínimos:** o `jokerd` corre como utilizador `joker`, com acesso por grupos (`gpio`, `dialout`, `i2c`, `spi`, `video`) e `CAP_SYS_NICE` + `CAP_IPC_LOCK` (para RT e `mlockall`). Não precisa de root. Os drivers tier X arrancam como **unidades systemd transitórias** (`StartTransientUnit`), com `DynamicUser` por omissão e a sandbox do systemd: é assim que correm com outro utilizador sem o `jokerd` ter `CAP_SETUID` (D20, `ref/06` §5.2).
- **Sandbox:** as apps Wasm não têm ambient authority (§9). Os drivers tier W só veem o transporte que lhes foi dado.
- **Rede:** a API local escuta no socket Unix por omissão, e o TCP é sempre mTLS (não há chave para o desligar). Os uplinks usam TLS com verificação de certificado obrigatória.
- **Comandos vindos de fora** (D16): as escritas pela API em TCP, pelo túnel de uma sessão remota e pelo protocolo do agente levam `nonce` e `ts` do emissor. Um nonce repetido é sempre rejeitado; com o relógio `synced`, também um `ts` fora de ±30 s. Sem relógio sincronizado, verifica-se só o nonce e a ligação ao alvo, e a auditoria regista-o. A memória de nonces nunca esquece: cheia, recusa pedidos (`ref/00` §9; documento 09, ataque 9). As **pontes** (OPC UA, Modbus, ROS 2) não levam nonce: autenticam-se pelo protocolo delas e escrevem como escritores declarados, com nível e lease.
- **Segredos** (ADR 0024): cada segredo é cifrado com HPKE para a chave X25519 de cada nó destinatário e assinado por entrada. O `secrets.toml.enc` só tem valores cifrados e **pode ir para o Git**; é assim que o Deck os distribui sem os ler. O nó guarda-os cifrados e decifra em memória fixada no momento do uso. As apps pedem segredos por um nome local ligado no manifesto (`[apps.<app>.secrets]`), só na instância async, e nunca os leem do manifesto (`ref/02` §2.4).

---

## 18. Tempo (M14)

- **Escalonamento:** `CLOCK_MONOTONIC`. Nunca o relógio de parede.
- **Timestamps:** UTC em ns, com **qualidade do relógio** por geração de tempo: `Synced(ntp|ptp|gnss)`, `Holdover(desde)`, `Unsynced`. Cada amostra guarda o `ts_acq` em monotónico relativo, e a tradução para UTC usa a tabela de correspondências do historian. Um salto de relógio (NTP a corrigir 3 horas depois de um arranque sem RTC) não baralha as séries: corrige-se retroativamente a correspondência, sem reescrever amostras.
- **Apps:** `cx.now()` devolve o tempo do tick (monotónico e UTC, mais a qualidade). Uma app pode declarar `requires_clock = "synced"`, e nesse caso as suas saídas ficam bloqueadas enquanto o relógio não estiver sincronizado: `Rejected(ClockUnsynced)` no passo 0 do envelope, e a saída vai para `safe` (§7; 16, E1). É o caso de uma tarifa horária.
- **PTP** (IEEE 1588 via `linuxptp`) para sites que precisam de correlação sub-ms entre dispositivos.

---

## 19. Observabilidade

Métricas próprias, expostas como sinais `joker.*` (portanto gravadas, alarmáveis e enviáveis como qualquer outro sinal) e via OTLP:

| Área | Métricas |
|---|---|
| Ciclo | jitter do arranque (histograma HDR), duração do tick, overruns, WCET medido por tarefa |
| Drivers | latência por pedido, timeouts, reconexões, bytes, pedidos coalescidos |
| Envelope | resultados por saída (`accepted/clamped/rejected/suppressed/tripped`) |
| Árbitro | nível ativo por saída, leases a expirar, overrides manuais ativos |
| Apps | traps, reinícios, memória, tempo de CPU async |
| ML | latência p50/p99 por modelo e backend, fallbacks de backend, OOD |
| Store/link | fila, lag do cursor, perdas por política, orçamento |
| Sistema | temperatura, throttling de CPU, disco, memória, qualidade do relógio |

`jk top` mostra tudo isto ao vivo, e `jk trace <sinal>` segue um sinal do driver ao envelope.

**Registo de decisões (regra normativa).** Regista-se por **mudança de resultado**, e não por tick: para cada saída, guarda-se cada mudança do triplo (valor aplicado, resultado, nível ativo), com a causa e a **contagem de repetições** do triplo anterior (quantos ticks seguidos deram o mesmo), e o instante da primeira e da última. Uma saída estável a 100 Hz gera uma entrada quando muda, e não 100 por segundo. O `fsync = "always"` (§12) só se aplica às entradas de **trips**, **escritas manuais**, **rejeições** (`Rejected(*)`) e **mudanças de nível ativo**; as outras (`Accepted`, `Clamped`, `Suppressed`, `Overridden` sem mudança de nível) vão no lote do WAL. O traço (16 §7) continua a ter todos os ticks: o registo é o resumo auditável, o traço é o replay. Para as saídas de um MCU, o registo aceita resultados **compactados**: quando a janela do Joker Link enche, os resultados pendentes juntam-se por (saída, nível), fica o mais recente, e os perdidos contam-se (`ref/07` §8.2). A telemetria nunca trava o controlo, e o que se perde é contado.

---

## 20. Modos de falha

| Falha | Deteção | Reação | Onde se vê |
|---|---|---|---|
| Equipamento deixa de responder | timeout do driver | sinais `Bad(CommFailure)`; permissivos dependentes a falso; saídas do equipamento: fallback do dispositivo | alarme `joker.device.<d>.offline` |
| Valor absurdo (NaN, fora dos limites físicos) | estágio de aquisição | `Bad(OutOfRange)` | qualidade |
| Sensor congelado com qualidade boa | `stuck_detect` | `Uncertain(Frozen)` | qualidade, alarme |
| Escrita que nunca volta | timeout por escrita | caixa latest-wins; N timeouts → `Down` → fallback do dispositivo | alarme |
| App em trap | host | propostas do tick descartadas; crashloop após K | alarme, `jk status` |
| App lenta no ciclo | epoch | interrompida; tick sem essa app | overrun da app |
| Grupo encravado | supervisor (deadline do grupo × 3) | o supervisor ativa o bit `force_safe` nas caixas das saídas do grupo, e os drivers enviam o `safe` sem passar pelo grupo | alarme crítico |
| `jokerd` morto | watchdog systemd / hardware | reinício; trips persistentes restaurados; saídas começam em `safe` | contador de arranques |
| Placa congelada | watchdog de hardware | reset; fallback do dispositivo cobre o intervalo | — |
| Modelo lento | deadline do modelo | `Err(Deadline)` para a app; fallback de backend após N | métricas ML |
| Driver de GPU encravado | watchdog do processo `joker-ml-<id>` | kill + reinício; fallback de backend | alarme |
| Disco cheio | store | retenção agressiva; sem bloquear ingestão; se `< 1%`, só alarmes e decisões | alarme |
| Rede do uplink em baixo | link | store-and-forward; orçamento recalculado | lag do cursor |
| Relógio salta | módulo de tempo | nova correspondência; `Holdover`/`Unsynced` | qualidade do relógio |
| Config nova corta a rede | janela de confirmação | rollback automático | registo de gerações |
| Falta de energia | — | no arranque: WAL, trips, níveis manuais (16, A14), última geração confirmada | — |

---

## 21. Números alvo (a validar)

| Métrica | Alvo | Condição |
|---|---|---|
| Binário `jokerd`, build `minimal` (`sim`, `sysfs`, Modbus, MQTT; `ref/06` §3.8) | ≤ 20 MB | aarch64, estático, `strip`; sem ML (a 1.0 não tem Burn, ADR 0019) |
| Binário `jokerd`, build completo (`default`, `ref/06` §3.8) | ≤ 30 MB | idem |
| RSS em repouso sem ML | ≤ 60 MB | 1 000 sinais, 3 apps, cache do historian de 8 MB (o `.text` residente conta) |
| Arranque até ao primeiro tick | ≤ 2 s | AOT em cache |
| Jitter do tick p99.9 | ≤ 100 µs | CM4/CM5, PREEMPT_RT, CPU isolada, grupo de 1 ms |
| Jitter do tick p99.9 | ≤ 2 ms | kernel sem RT, grupo de 100 ms |
| Overhead da chamada Wasm de ciclo | ≤ 5 µs | tarefa vazia, Cranelift |
| Envelope + árbitro por saída | ≤ 300 ns | sem grupos atómicos |
| Ingestão de amostras escalares | ≥ 100 k/s | CM4, 1 núcleo `io` |
| Ingestão do historian | ≥ 200 k amostras/s | CM4, eMMC |
| Hot-swap de app | 0 ticks perdidos | mesma assinatura de portas |

**Tamanho do binário na CI.** Os dois builds (`minimal` e completo) medem-se em cada commit com `cargo bloat --release --crates` para `aarch64-unknown-linux-gnu`; um build acima do limite da tabela faz a CI falhar, com os crates que mais cresceram. Subir um limite é uma decisão explícita, no mesmo commit que o justifica.

---

## 22. Plataformas

| Tier | Plataforma | Estado previsto |
|---|---|---|
| **1** | Linux aarch64: Raspberry Pi 4/5, CM4/CM5, RevPi Connect 4/5, Jetson Orin | CI com hardware, RT medido |
| **1** | Linux x86_64: PCs industriais | CI com hardware |
| **2** | Linux aarch64 sem hardware em CI: Raspberry Pi 3 B+ (a bancada, documento 23) | Cranelift; RT medido na bancada, sem alvo |
| **2** | Linux armv7: RevPi Core 3, gateways antigos | Pulley em vez de Cranelift; sem ML em GPU |
| **2** | Linux riscv64 | Cranelift; sem hardware em CI |
| **dev** | macOS, Windows (WSL2) | drivers `sim`, Burn Metal/Vulkan; nunca em produção |
| **dev** | Linux armv6: Raspberry Pi Zero W (bancada) | Pulley, se o wasmtime compilar para ARMv6 (a confirmar na Fase 1); senão só apps nativas, e o `jk check` avisa com o `J0243` (ref/01 §10.2); 1 núcleo, sem RT |
| **dev (MCU)** | ESP32 original (Xtensa LX6, bancada) | Joker Micro para desenvolvimento; toolchain Xtensa da Espressif |
| **1 (MCU, M1)** | RP2350, STM32G4 | **Joker Micro**: o mesmo núcleo `no_std`, Embassy, apps Rust nativas, Joker Link (documento 19) |
| **2 (MCU, M2)** | ESP32-C6/S3, STM32H5/H7 | Joker Micro com rede (Wi-Fi, Ethernet): nós autónomos |
