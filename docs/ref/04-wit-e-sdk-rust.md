# ref/04 — O mundo WIT `joker:runtime`, a secção `joker:meta`, o SDK Rust e o `joker-test`

Referência normativa das interfaces que uma app vê. Fica acima dos documentos 01–05 (docs/README, ADR 0023) e aplica o `ref/00`. A semântica (árbitro, envelope, qualidade, expressões) é a do documento 16 e não se repete aqui.

---

## 1. Âmbito e fontes

### 1.1 O que cobre

| Superfície | Onde | Secção |
|---|---|---|
| mundo WIT das apps, `joker:runtime@0.1.0` (→ `@1.0.0` na 1.0) | `wit/joker-runtime.wit` | §2 |
| ABI de ciclo: imagem de entradas e buffer de propostas, byte a byte | este documento | §3 |
| SDK Rust `joker-sdk` (importado como `joker`): macros, tipos, traits, backends Wasm e nativo | este documento | §4 |
| secção custom `joker:meta` | `spec/cddl/joker-meta.cddl` | §5 |
| harness de testes `joker-test` | este documento | §6 |
| erros de runtime `J-E…` vistos pelas apps, e erros de compilação do SDK | este documento | §7, §4.16 |
| cópia máquina-legível do mundo `joker:sim/plant` (definido no ref/03, §2.4.10) | `wit/joker-sim.wit` | §2.10 |

Fica fora: os SDKs de tier 2 (ref/05), o mundo `joker:driver` (ref/06), a ABI C e o crate do Joker Micro (ref/08), a API HTTP (ref/09).

### 1.2 Fontes

Documentos 04 (todo), 14 §3 e §6–7, 17 §2 e §4–6, 16 (todo), 19 §3 e §5, 02 §4–8, 01 §3–5, §9 e §18, 09 (ataques 2, 6, 8 e 10), 11 ("Preparado para ML"), 21 §1 e §3, ADR 0004, 0020 e 0022, e os 34 blocos `rust` de `docs/**/*.md` (§9).

### 1.3 Ligações a outras referências

| Referência | O que partilha com esta |
|---|---|
| ref/00 | códigos de qualidade (§4.3) e de resultados (§4.4), tempo (§3), `record error` (§8), limites (§10) |
| ref/01 | `[apps.<app>]`: `bind`, `params`, `cycles`, `secrets`, `requires_clock`, `resources.*_memory`, `net.allow`; `[instances.*] implemented_by` |
| ref/02 | `joker.lock` `[apps.<app>] wit` lido de `joker:meta`; segredos declarados em `joker:meta` e ligados em `[apps.<app>.secrets]` |
| ref/03 | cenários e casos de conformidade de classe, que o `joker-test` executa (§6.7); literais (§2.1.3); simuladores de classe |
| ref/05 | os SDKs de tier 2 usam o mesmo mundo, a mesma imagem empacotada e o mesmo buffer de propostas (§3), as mesmas funções de `cycle-io` para o acesso dinâmico e o mesmo esquema de `joker:meta` (com as chaves `tier`, `lang`, `engine` e `sdk`, §5); a correspondência função a função está na ref/05 §16 |
| ref/06 | `joker:driver` importa `joker:runtime/log` com a mesma versão (§2.5.3) |
| ref/08 | o backend nativo do SDK (feature `backend-native`, contrato `NativeApp`, ref/08 §2.11.5) usa o mesmo buffer de propostas (§3.3) e a mesma secção de metadados (§5.1) |
| ref/09, ref/10 | a API e o agente mostram `joker:meta` e recusam, antes do deploy, mundos mais novos do que a runtime (`J-E0001`) e funcionalidades instáveis em `prod` (`J-E0003`); a API mostra os erros de `invoke` com os códigos do §7.1 e os alarmes das apps com o id `app.<app>.<nome>` (§4.2) |

### 1.4 Validação dos ficheiros máquina-legíveis

- `wit/joker-runtime.wit` e `wit/joker-sim.wit` foram validados com o analisador WIT do `wasm-tools` (via `jco embed --dummy` + `jco new`), em duas passagens: o mundo estável (sem as funcionalidades `@unstable`) e o mundo com todas as funcionalidades ativas. `wit/joker-driver.wit` (ref/06) resolve contra este pacote.
- `spec/cddl/joker-meta.cddl` foi validado com o `zcbor` contra instâncias CBOR de exemplo (a de §5.5 e duas das apps do documento 04), e rejeita instâncias com chaves obrigatórias em falta ou valores fora das escolhas. O `zcbor` não suporta o controlo `.regexp` (RFC 8610, §3.8.3), que foi retirado só para a validação.

---

## 2. O mundo WIT `joker:runtime`

### 2.1 Visão geral

Uma app é um componente (ADR 0002) instanciado **duas vezes** (documento 01, §5; ADR 0004): uma instância de ciclo (thread `rt/<grupo>`) e uma instância async (tokio `app`). As importações estão todas no mundo; o host liga-as às duas instâncias, e as que não pertencem a uma delas fazem trap.

| Interface | Instância de ciclo | Instância async | Fase |
|---|---|---|---|
| `types` | tipos só | tipos só | 1.0 |
| `cycle-io` | sim | trap `J-E0902` | 1.0 (`write-at`: 1.1) |
| `async-io` | trap `J-E0901` | sim | 1.0 (`write-at`, `propose`: 1.1) |
| `kv` | trap `J-E0901` | sim | 1.0 |
| `secrets` | trap `J-E0901` | sim | 1.0 |
| `log` | sim | sim | 1.0 |
| `guest` (exportada) | `init`, `on-params`, `on-cycle`, `export-state`, `migrate` | `init`, `on-params`, `run-task`, `on-action`, `export-state`, `migrate` | 1.0 |
| `ml` | trap `J-E0901` | sim | **reservada, 1.1** (`@unstable(feature = ml)`) |
| `frames` | trap `J-E0901` | sim | **reservada, 1.1** (`streams`) |
| `blocks` | trap `J-E0901` | sim | **reservada, 1.1** (`blocks`) |

Um export chamado na instância errada não acontece: o host nunca chama `on-cycle` na instância async nem `run-task` na de ciclo. Uma app sem tarefas async não tem instância async; uma app sem tarefas de ciclo não tem instância de ciclo.

As apps de tier 2 (TypeScript, Python) também têm tarefas de ciclo, com períodos mínimos de 100 ms (TS) e 500 ms (Python) e admissão pelo **máximo** medido (D15, ref/05 §19). Usam o mesmo mundo e a mesma ABI de ciclo (§3).

### 2.2 `types`

| Tipo | Definição | Notas |
|---|---|---|
| `port` | `u32` | índice em `init-context.ports` (§2.6.2); local à app e à geração |
| `instance` | `u32` | índice em `init-context.instances` |
| `method` | `u16` | posição do método na lista `methods` do dispositivo em `joker:meta` (§5) |
| `level` | `u8` | nível do árbitro, 1–16 |
| `quality-code` | `enum { good, uncertain, bad }` | fechado para sempre |
| `quality-reason` | `enum` com 20 casos | ordem dos casos = código do ref/00 §4.3 (`none` 0 … `deadline` 19) |
| `quality` | `record { code, reason }` | |
| `value` | `variant { boolean(bool), int(s64), uint(u64), float(f64), text(string), blob(list<u8>) }` | sempre na unidade da porta; `enum` e `bits` como `uint`; `f32` promovido; registos como `blob` CBOR (§4.10) |
| `sample` | `record { value, quality, ts-src: option<s64>, ts-hw: option<u64>, ts-acq: u64, seq: u64 }` | tempos do ref/00 §3 |
| `clock-quality` | `enum { synced, holdover, unsynced }` | ref/00 §6 |
| `instant` | `record { mono-ns: u64, utc-ns: s64, clock }` | |
| `clamp-kind` | `enum { range, rate }` | |
| `clamp` | `record { by: clamp-kind, applied: value }` | |
| `reject-reason` | `enum` com 8 casos | ordem = código do ref/00 §4.4 menos 1 |
| `write-outcome` | `variant` (abaixo) | `none` exprime-se como `option::none` |
| `error` | `record { code: string, message: string }` | ref/00 §8; `code` é um código do registo, `message` é texto em inglês que ninguém interpreta |
| `change` | `record { port, sample }` | |
| `event-delivery` | `record { port, payload: list<u8>, seq: u64, at: instant, lost: u32 }` | `lost`: eventos perdidos por este consumidor desde a entrega anterior |
| `member-outcome` | `record { port, outcome: write-outcome }` | uma escrita expandida de um método |

**`write-outcome` e os códigos do ref/00 §4.4:**

| Caso WIT | Código | JSON (`outcome`) | Rust (`WriteOutcome<T>`) |
|---|---|---|---|
| (ausente: `option::none`) | 0 | `"none"` | `None` em `Option<WriteOutcome<T>>` |
| `accepted` | 1 | `"accepted"` | `Accepted` |
| `clamped({ by: range, applied })` | 2 | `"clamped_range"` | `Clamped { by: ClampKind::Range, applied }` |
| `clamped({ by: rate, applied })` | 3 | `"clamped_rate"` | `Clamped { by: ClampKind::Rate, applied }` |
| `suppressed` | 4 | `"suppressed"` | `Suppressed` |
| `overridden(level)` | 5 | `"overridden"` + `level` | `Overridden { level }` |
| `deferred` | 6 | `"deferred"` | `Deferred` |
| `shadowed` | 7 | `"shadowed"` | `Shadowed` |
| `rejected(reason)` | 8 | `"rejected"` + `reason` | `Rejected(RejectReason)` |
| `tripped` | 9 | `"tripped"` | `Tripped` |

`reject-reason`: `wrong-type` 1, `interlock` 2, `quality` 3, `range` 4, `atomic-group` 5, `not-owner` 6, `clock-unsynced` 7, `budget` 8.

### 2.3 `cycle-io` (instância de ciclo)

Todas as funções são síncronas, não alocam no host e não bloqueiam. **Todos os SDKs**, o Rust e os de tier 2, usam o caminho empacotado (§3; ref/05 §16, A11) e só chamam `read` (carimbos `ts_src`/`ts_hw`, valores de arena e acesso dinâmico por nome), `mailbox-send`, `mailbox-recv` e `random-u64`. As restantes funções existem para acesso dinâmico e para valores que não cabem em `u64`; não são o caminho normal de nenhum SDK.

| Função | Semântica | Equivalente empacotado | Erros |
|---|---|---|---|
| `read(p) -> sample` | amostra do snapshot do latch deste tick; numa saída, o valor **aplicado** (02 §6.1) | entrada na secção de amostras | porta sem amostra (ação, evento, mailbox): trap `J-E0907` |
| `write(p, v)` | acrescenta uma proposta | registo `WRITE` | tipo errado: a proposta sai `rejected(wrong-type)`; porta que não é saída: trap `J-E0907` |
| `relinquish(p)` | liberta já o nível deste escritor (01 §6, regra 3) | `WRITE` com `RELINQUISH` | como `write` |
| `pending(p) -> option<value>` | última proposta desta app para `p` neste tick, feita no domínio de ciclo (as tarefas anteriores do mesmo grupo contam; as propostas async drenadas no passo 2 não) | o SDK sabe-o sozinho | — |
| `last-outcome(p) -> option<write-outcome>` | resultado da proposta **deste escritor** no commit mais recente do grupo dono; `none` se não teve proposta nesse commit | secção de resultados | — |
| `param(p) -> value` | valor atual do parâmetro | entrada na secção de amostras | — |
| `call(inst, m, args)` | acrescenta um registo de método (17 §4.1) | registo `METHOD` | método ou instância fora da gama: trap `J-E0907`; argumentos inválidos: `J-E0504` no registo de decisões, sem trap |
| `publish(p, v)` | escreve um sinal publicado (`#[publish]`), a condição `bool` de um alarme da app (id `app.<app>.<nome>`, §4.2), um `Internal<T>` com escritor de ciclo (`blob` CBOR), ou um membro `state` de uma implementação | registo `PUBLISH` | porta errada: trap `J-E0907` |
| `counter-add(p, delta)` | soma a um `Counter` | registo `COUNTER_ADD` | idem |
| `emit(p, payload)` | emite um evento; filas de consumidores cheias perdem e contam | registo `EMIT` | payload > 8 KiB: `J-E0410`, o evento é descartado |
| `note-substituted(p)` | regista no traço que a leitura de `p` foi substituída (`get_or`, 16 §3.1) | registo `NOTE` | — |
| `mailbox-send(p, msg) -> bool` | envio não bloqueante para a instância async; `false` se cheia (perda contada) | — | mensagem > 256 B: `false` e `J-E0412` no log |
| `mailbox-recv(p) -> option<list<u8>>` | receção não bloqueante | — | — |
| `now() -> instant` | instante do latch deste tick (02 §5) | cabeçalho da imagem | — |
| `dt-ns() -> u64` | período efetivo do grupo | cabeçalho da imagem | — |
| `random-u64() -> u64` | pseudoaleatório determinista (§2.7) | — | — |
| `write-at(p, v, at)` | **1.1** (`scheduled-writes`, M6) | `WRITE` com `AT` | |

### 2.4 `async-io` (instância async)

| Função | Semântica | Erros (`error.code`) |
|---|---|---|
| `read(p) -> sample` | valor atual do slot, sem latch (02 §6.4) | — |
| `snapshot(ps) -> result<list<sample>, error>` | leitura coerente (mesma época seqlock); ≤ 256 portas | `J-E0414` acima do limite |
| `set(p, v)` | proposta sem esperar: vai para a inbox do grupo dono; `last-outcome` dá `deferred` até ao commit | — |
| `write(p, v) -> result<write-outcome, error>` (async) | proposta; o future resolve depois do commit do grupo dono (01 §5, o `WriteTicket`) | `J-E0206` a geração mudou antes do commit; `J-E0205` escritor só-proposta |
| `relinquish(p)` | liberta o nível | — |
| `last-outcome(p)` | como em `cycle-io` | — |
| `param(p) -> value` | valor atual | — |
| `call(inst, m, args) -> result<list<member-outcome>, error>` (async) | método de classe; resolve depois do commit com um resultado por escrita expandida (17 §2.1) | `J-E0504`, `J-E0505`, `J-E0206` |
| `invoke(p, args, key) -> result<list<u8>, error>` (async) | ação (M19, 17 §4.2): permissivo e papel → deduplicação pela chave → fila serializada do dispositivo → resultado | `J-E0301`…`J-E0313`; na API, os do §7.1 |
| `start-action(p, args, key) -> result<action-call, error>` | ação `@long`: devolve logo um recurso `action-call` | idem; `J-E0313` se a ação não for longa |
| `action-call.progress() -> stream<action-progress>` | eventos de progresso | — |
| `action-call.cancel()` | pede o cancelamento | — |
| `action-call.finish() -> result<list<u8>, error>` (async) | espera pelo resultado | `J-E0308` cancelada |
| `publish`, `counter-add`, `emit`, `note-substituted` | como em `cycle-io`; vão pela inbox do grupo dono do sinal | como em `cycle-io` |
| `subscribe(ps) -> result<stream<change>, error>` | uma mudança por `seq` nova; ≤ 256 portas; com o stream cheio (64), fica só a última mudança de cada porta, e as perdas contam | `J-E0414` |
| `events(p) -> result<stream<event-delivery>, error>` | fila de consumidor própria, profundidade 64 | `J-E0414` |
| `mailbox-send`, `mailbox-recv` | como em `cycle-io`, no sentido inverso | — |
| `mailbox-next(p) -> list<u8>` (async) | espera pela próxima mensagem | — |
| `history(q) -> result<list<sample>, error>` (async) | historian (01 §12); `step-ns = 0` só com `raw`; ≤ 10 000 pontos; com agregado, uma amostra por passo, com a **qualidade dominante** do passo (01 §12) e `ts-acq` = início do passo | `J-E0401` intervalo inválido; `J-E0402` resultado grande de mais; `J-E0403` sinal sem gravação |
| `now() -> instant` | instante atual | — |
| `sleep-until(mono-ns)` (async) | acorda no instante monotónico dado (ou logo, se já passou) | — |
| `random-u64()` | §2.7 | — |
| `report-progress(request, fraction, message)` | progresso de uma ação que esta app serve em `on-action` | — |
| `is-cancelled(request) -> bool` | o chamador cancelou a ação servida | — |
| `write-at(p, v, at)` | **1.1** (`scheduled-writes`) | `J-E0207` fila de agendamento cheia |
| `propose(p, v, reason, ttl-ns) -> result<string, error>` | **1.1** (`intents`, M18); devolve o ULID da intenção, que expira ao fim de `ttl-ns` se ninguém a decidir (ref/09 §5.9) | `J-E0209`, `J-E0210` |

`history-query { port, from-utc-ns, to-utc-ns, step-ns, agg }` com `agg ∈ { raw, mean, min, max, first, last, count }`: os agregados do historian (01 §12). `count` devolve `uint`.

### 2.5 `kv`, `secrets` e `log`

#### 2.5.1 `kv` (só async)

| Função | Semântica | Erros |
|---|---|---|
| `get(key) -> result<option<list<u8>>, error>` | leitura | `J-E0405` chave inválida |
| `set(key, value)` (async) | escrita atómica e durável (fsync) antes de resolver | `J-E0404` quota; `J-E0405`; `J-E0406` valor grande de mais; `J-E0407` erro de armazenamento |
| `delete(key)` (async) | idem | idem |
| `keys(prefix) -> result<list<string>, error>` | por ordem bytewise | — |
| `commit(ops)` (async) | até 64 operações, todas ou nenhuma | idem, e `J-E0404` |

Limites: chave 1–128 B UTF-8 sem caracteres de controlo; valor ≤ 64 KiB; total ≤ 1 MiB por app em Linux (01 §9.1) e ≤ 16 KiB no Joker Micro (19 §3).

#### 2.5.2 `secrets` (só async)

`reveal(name) -> result<list<u8>, error>`. `name` é o nome local declarado pela app (§4.2) e ligado no manifesto (ref/02 §2.4.7). Um nome não declarado ou não ligado dá `J-E0408`; declarado, ligado mas ausente no cofre do nó, `J-E0409`. O valor nunca entra no traço, no log nem no registo de decisões; no replay, a app recebe um valor substituto fixo (ref/02).

#### 2.5.3 `log`

`log(lvl, msg, fields)` e `enabled(lvl) -> bool`. Não bloqueia: cada instância tem um anel de 256 entradas; cheio, perde e conta em `joker.app.<app>.log_dropped`. `msg` é truncada a 1 KiB, `fields` a 16 pares de 256 B cada. Limite de ritmo: 100 mensagens/s por instância (as restantes contam como perdidas).

A interface é **autónoma** (não usa `types`) de propósito: `joker:driver` importa-a com a mesma versão do pacote (ref/00 §1, ref/06 §4.1).

### 2.6 `guest` (exportada pela app)

#### 2.6.1 Os exports

| Export | Instância | Quando | Prazo | Erro devolvido |
|---|---|---|---|---|
| `init(cx) -> result<_, error>` | ambas | ao criar a instância, antes do primeiro tick ou gatilho | 1 s | `Err` ou prazo: a instância não arranca (`J-E0909`); numa troca a quente, a versão anterior continua (01 §9) |
| `on-params(changes) -> result<_, error>` | a async, se existir; senão a de ciclo, num limite de tick | quando uma geração nova muda parâmetros (ref/01 §5.12.2) | 100 ms | `Err`: a geração é recusada para esta app (`J-E0011`), e o `jk apply` faz rollback (01 §15) |
| `on-cycle(task, image) -> cycle-result` | ciclo | uma vez por tarefa e por tick, passo 4 (01 §4) | deadline por epoch (01 §4) | `cycle-result.error` termina a tarefa neste tick **sem** descartar as propostas já no buffer; trap descarta-as. `error.code = J-E0816` (exceção não apanhada num SDK de tier 2, ref/05 §9.3) vem com `proposals` vazio e o host trata-o como um trap (descarta as propostas da app nesse tick e conta para o crashloop), mas **sem** recriar a instância |
| `run-task(task, why) -> result<_, error>` | async | uma chamada por disparo do gatilho (§4.3.2), também em `on` e `on_event` | — | `Err`: registado e contado em `joker.app.<app>.task.<t>.errors`; `J-E0816` conta em `joker.app.<app>.task.<t>.failures` e para o crashloop |
| `on-action(req) -> result<list<u8>, error>` | async | ação de uma instância virtual servida por esta app (17 §5.3) | o `timeout` da ação | `Err` chega ao chamador tal como está; trap dá `J-E0312` ao chamador |
| `export-state() -> result<list<u8>, error>` | ambas | troca a quente da versão antiga | 100 ms | `Err`: a nova versão começa com o estado inicial |
| `migrate(previous, from-version) -> result<_, error>` | ambas | troca a quente da versão nova, depois de `init` | 1 s | `Err`: a versão antiga continua (`J-E0914`) |

`cycle-result { proposals: list<u8>, error: option<error> }` existe porque um `result<list<u8>, error>` perderia as propostas de uma tarefa que devolve `Err`, e o documento 04 §3.2 manda mantê-las.

`trigger`: `startup`; `timer(instant)` para `every` e `at`; `change(change)` para `on = "<porta>"`; `event(event-delivery)` para `on_event`.

`action-request { request: u64, action: u32, args, idempotency-key, caller, deadline-mono-ns }`: `action` é o índice do membro na lista de membros de `implements` em `joker:meta` (§5); `caller` é o id do escritor (`user:ops`, `app:station`, ref/00 §5).

#### 2.6.2 `init-context` e a tabela de portas

| Campo | Conteúdo |
|---|---|
| `kind` | `cycle` ou `task` |
| `mode` | `normal`, `shadow` (M16: as escritas saem `shadowed`), `replay`, `test` |
| `app`, `app-version`, `node` | identidade |
| `served-world` | mundo servido depois do adaptador (§2.8) |
| `generation`, `generation-hash` | geração (ref/00 §5) |
| `seed` | semente do gerador (§2.7) |
| `ports` | a tabela de portas (abaixo) |
| `instances` | uma entrada por instância de classe ligada, e uma por nó da árvore de membros de classe, em profundidade (§4.8.6): `device`, `elem`, `name` (o nome da instância, ou o caminho aninhado de um submembro sem nome), `class-version`, `capabilities` |
| `tasks` | as tarefas ativas nesta instância, com o grupo e o período efetivo |
| `implements` | nome da instância virtual servida por esta instância da app |
| `cycle-abi`, `image-len`, `proposal-capacity` | ABI de ciclo (§3.1) |

**A tabela de portas** é construída pelo host, sempre pela mesma ordem, e o `port` de uma porta é a sua posição:

1. cada porta de `joker:meta` (chave 6), pela ordem da lista, que é a que o SDK escreveu (a de declaração dos campos em Rust; a dos nomes nos SDKs de tier 2, ref/05 §15.1); uma porta `many` ocupa `n` posições seguidas (`n` fixado no `jk check`, pela ordem do `bind`: a da lista, ou a lexicográfica de um seletor);
2. cada dispositivo (chave 8), pela ordem de declaração; para cada elemento (1, ou `n` num `Many`); para cada membro da lista de membros **do handle** (`joker:meta`, e não do contrato em runtime), uma posição; um membro de classe não ocupa posição, e desce-se à sua lista de membros, em profundidade (§4.8.6);
3. os membros de `implements` (chave 9), pela ordem da lista.

Cada `port-binding` diz a origem (`declared`, `device-member`, `command`, `state`, `class-event`), a declaração, o elemento, o membro, o nome resolvido, se está ligada (`bound`) e onde fica na imagem (`slot`, `outcome`). Um membro que o handle conhece e a instância não tem (capacidade em falta, membro opcional sem mapeamento, classe de minor anterior) fica com `bound = false`: a leitura dá `J-E0506`, e uma escrita é descartada com `J-E0506` no registo.

Como a lista de membros vem de `joker:meta`, uma app gerada contra `pump@1.2` corre sobre uma instância `pump@1.0` sem desalinhar índices (A11 do documento 14).

### 2.7 Relógio, aleatoriedade e WASI

- `now()` e `dt-ns()` da instância de ciclo são os do latch; o replay é exato (02 §5).
- **Aleatoriedade:** `random-u64` usa, por instância, um xoshiro256\*\* semeado com `seed` = os primeiros 8 bytes (LE) de `SHA-256("joker-seed" ‖ generation-hash ‖ app ‖ kind ‖ implements)`. Determinista, não gravado no traço (reproduz-se a partir da geração) e **não criptográfico**.
- **WASI.** Um componente Rust, TS ou Python importa quase sempre interfaces WASI (o `HashMap` da `std` pede `wasi:random/insecure-seed`). O host satisfaz estas, em 0.2.x e 0.3.x, sempre virtualizadas:

| Interface WASI | Ciclo | Async |
|---|---|---|
| `wasi:clocks/monotonic-clock`, `wall-clock` | instante do latch | instante atual |
| `wasi:random/random`, `insecure`, `insecure-seed` | o mesmo gerador de `random-u64` | idem |
| `wasi:cli/environment`, `stdin` | vazios | vazios |
| `wasi:cli/stdout`, `stderr` | `log` `info` / `warn`, por linha, truncado a 1 KiB | idem |
| `wasi:cli/exit` | trap `J-E0905` | trap `J-E0905` |
| `wasi:filesystem` | sem diretórios pré-abertos | idem |
| `wasi:sockets`, `wasi:http/outgoing-handler` | sempre `access-denied` | filtrado por `net.allow` (ref/01); fora da lista, `access-denied` e `J-E0413` no log |
| `wasi:io` | sim | sim |

Qualquer outra importação faz o host recusar o componente ao carregar (`J-E0006`).

### 2.8 Evolução dos enums e adaptadores

- `quality-reason`, `reject-reason`, `clamp-kind` e `write-outcome` só crescem **no fim**. Cada caso novo é introduzido por uma ADR que declara a sua **projeção** no caso existente que uma app de um mundo anterior vê. Exemplos: um motivo de qualidade novo projeta-se em `none` com o mesmo `quality-code` (ref/00 §1, regra 2: conta pela sua categoria); um motivo de rejeição novo projeta-se no motivo existente mais próximo que a ADR indicar.
- O host conhece o mundo de cada app (chave 3 de `joker:meta`) e aplica a projeção **antes** de escrever a imagem e antes de devolver valores WIT. Uma app nunca recebe um caso que o seu mundo não declara, nem pelo caminho empacotado (onde os códigos são bytes e o tipo não o impediria).
- **Janela de adaptadores:** até à 1.0, a runtime serve o mundo atual e o minor anterior (M21, documento 09, ataque 8). A partir da 1.0, serve **todos** os `joker:runtime@1.y` com y ≤ o seu (regra de ouro, ADR 0022; documento 21, §1.1). Um mundo mais novo do que a runtime é recusado antes do deploy pelo `jk` e pelo agente (`J-E0001`), e, se chegar ao nó, ao carregar.

### 2.9 Versões e reservas

- O pacote é `joker:runtime@0.1.0` até à 1.0 e `joker:runtime@1.0.0` na 1.0 (ref/00 §1). Um item sem porta existe desde a primeira versão do pacote; um item acrescentado numa minor leva `@since(version = 1.y.0)`; nada se remove numa major.
- **Reservas da 1.1** usam `@unstable(feature = <nome>)` no próprio pacote:

| Funcionalidade | O que reserva | Documento |
|---|---|---|
| `ml` | interface `ml` (tensores, `infer`) | 06, M5 e M17 |
| `streams` | interface `frames` | 01 §11, M4 |
| `blocks` | interface `blocks` | 01 §11, M7 |
| `scheduled-writes` | `cycle-io.write-at`, `async-io.write-at`, flag `AT` do registo `WRITE` | 02 §7.2, M6 |
| `intents` | `async-io.propose` | 01 §6.1, M18 |

- Um item `@unstable` **não faz parte do mundo estável**: as ferramentas geram os bindings sem ele, a menos que a funcionalidade seja pedida (no SDK Rust, uma feature `unstable-<nome>`, §4.15). Um componente que importa um item instável é recusado pelo `jokerd` em `mode = "prod"` (`J-E0003`). Em `mode = "dev"`, a runtime 1.0 liga-o a stubs que devolvem `J-E0601`, o que deixa desenvolver contra a forma prevista.
- Na 1.1, cada item estabilizado troca `@unstable(feature = …)` por `@since(version = 1.1.0)`. A forma pode ainda mudar antes dessa troca, e por isso **uma app compilada com uma funcionalidade instável não está coberta pela regra de ouro**. É isto que permite reservar já as interfaces (documento 11, "Preparado para ML") sem que a forma escolhida hoje fique presa para toda a série 1.

### 2.10 Outros pacotes em `wit/`

| Ficheiro | Pacote | Dono |
|---|---|---|
| `wit/joker-runtime.wit` | `joker:runtime@0.1.0`, mundo `app` | esta referência |
| `wit/joker-driver.wit` | `joker:driver@0.1.0`, mundo `driver`; importa `joker:runtime/log@0.1.0` | ref/06 |
| `wit/joker-sim.wit` | `joker:sim@0.1.0`, mundo `plant` | ref/03, §2.4.10 (este ficheiro é a cópia máquina-legível; se divergirem, vale o ref/03) |

### 2.11 Plano B para o async do tier 2 (D15)

Se o async do WASI 0.3 não estiver estável no ComponentizeJS ou no componentize-py quando os SDKs de tier 2 saírem, o pacote ganha um segundo mundo, **`app-blocking`** (nome já reservado no `wit/joker-runtime.wit`):

- as mesmas interfaces e as mesmas funções, com as mesmas assinaturas, sem `async`: `write`, `call`, `invoke`, `history`, `mailbox-next`, `sleep-until`, `kv.set`, `kv.delete`, `kv.commit` e `action-call.finish` bloqueiam até ao resultado; os `stream<T>` passam a recursos com `next() -> option<T>` bloqueante;
- o host serve a instância async desse componente numa **thread dedicada da pool `app`**, nunca no domínio de ciclo; a instância de ciclo é igual à do mundo `app` (as suas funções já são síncronas);
- perde-se só a concorrência entre tarefas async da mesma app: uma `history()` lenta atrasa as outras tarefas async dessa app (ref/05 §29, D9);
- o `joker:meta` diz o mundo (chave 3); o `jk check` e o agente tratam `app-blocking` como o `app` da mesma versão para a janela de adaptadores (§2.8).

A decisão de ativar o plano B é uma ADR na Fase 3, com a prova de que o async não está pronto. O SDK Rust usa sempre o mundo `app`.

---

## 3. A ABI de ciclo (layout binário)

O caminho rápido do documento 09, ataque 2: **uma chamada por tarefa e por tick**, `on-cycle(task, image) -> cycle-result`. Todos os inteiros são little-endian, sem alinhamento implícito (os leitores usam `from_le_bytes`), e os campos reservados são 0 à escrita e ignorados à leitura.

### 3.1 Versão e negociação

- A app declara a versão da ABI que o seu SDK fala em `joker:meta` (chave 5). A v1 só tem a versão **1**.
- O host escolhe a versão e comunica-a em `init-context.cycle-abi`, junto com `image-len` (fixo durante a geração) e `proposal-capacity`.
- Uma versão nova acrescenta campos no fim do cabeçalho (o cabeçalho diz o seu comprimento) e tipos de registo novos. Um host lê sempre as versões anteriores. Um SDK que declara uma versão que o host não conhece é recusado ao carregar (`J-E0002`).

### 3.2 A imagem de entradas

```
imagem := cabeçalho (64 B) · amostras (24 B × n_slots) · resultados (24 B × n_outcomes) · secção larga
```

**Cabeçalho (64 B):**

| Offset | Tamanho | Campo | Notas |
|---|---|---|---|
| 0 | 1 | `abi` | 1 |
| 1 | 1 | `header_len` | 64 |
| 2 | 2 | `flags` | bit 0 `FIRST_TICK` (primeiro tick desta instância nesta geração); bit 1 `SHADOW`; bit 2 `REPLAY`; bit 3 `AFTER_SKIP` (o tick anterior foi saltado por `overrun = "skip"`); bits 4–15 reservados |
| 4 | 4 | `task` | índice da tarefa em `joker:meta` |
| 8 | 8 | `tick_seq` | número do tick do grupo |
| 16 | 8 | `t_mono_ns` | instante do latch |
| 24 | 8 | `t_utc_ns` (i64) | idem, em UTC |
| 32 | 8 | `dt_ns` | período efetivo do grupo |
| 40 | 1 | `clock` | `clock_quality` (ref/00 §6) |
| 41 | 3 | reservado | |
| 44 | 4 | `n_slots` | |
| 48 | 4 | `n_outcomes` | |
| 52 | 4 | `outcomes_off` | = 64 + 24 × `n_slots` |
| 56 | 4 | `wide_off` | início da secção larga |
| 60 | 4 | `wide_len` | |

**Amostra (24 B), uma por porta com `slot`:** entradas, saídas (valor aplicado), parâmetros, `Internal<T>`, o estado de cada `Alarm` da app (sinal `alarm.app.<app>.<nome>.state`, §4.2), consultas de classes, membros `in`/`out`/`param` de dispositivos e os comandos de uma implementação, pela ordem de `slot`.

| Offset | Tamanho | Campo | Notas |
|---|---|---|---|
| 0 | 8 | `value` | bits do valor (ref/00 §4.1); com `WIDE`, 32 bits baixos = offset desde o início da imagem, 32 altos = comprimento |
| 8 | 1 | `quality_code` | ref/00 §4.3 |
| 9 | 1 | `quality_reason` | ref/00 §4.3, já projetado (§2.8) |
| 10 | 2 | `flags` | bit 0 `WIDE`; bit 1 `HAS_TS_HW`; bit 2 `HAS_TS_SRC`; bit 3 `CHANGED` (a `seq` mudou desde o latch anterior deste grupo); bit 4 `UNBOUND`; bits 5–15 reservados |
| 12 | 4 | `seq` | 32 bits baixos; o SDK estende-os como no documento 01, §3.2 |
| 16 | 8 | `ts_acq` | ns monotónicos |

Os bytes 8–11, lidos como `u32`, têm a disposição da palavra de qualidade do slot (código nos bits 0–7, motivo nos 8–15, flags nos 16–31, documento 01 §3.2). O `ts_hw` e o `ts_src` não vão na imagem: `HAS_TS_HW`/`HAS_TS_SRC` dizem ao SDK que `sample()` tem de os ir buscar com `cycle-io.read(p)`, uma chamada sem alocação no host.

Uma porta por ligar (`UNBOUND`) tem `quality = bad(not_connected)`. Uma amostra que nunca chegou tem `uncertain(initial)` e `value = 0`.

**Resultado (24 B), um por porta de saída com `outcome`:**

| Offset | Tamanho | Campo | Notas |
|---|---|---|---|
| 0 | 1 | `outcome` | código do ref/00 §4.4 (0 = `none`), já projetado |
| 1 | 1 | `detail` | `rejected`: motivo (1–8); `overridden`: nível ativo; senão 0 |
| 2 | 1 | `winner` | nível ativo que ganhou (1–16); 0 = nenhum (a saída está em `safe`) |
| 3 | 1 | `flags` | bit 0 `MINE_ACTIVE` (o lease deste escritor está ativo); bit 1 `TRIPPED`; bit 2 `OTHER_GROUP` (o grupo dono é outro: o resultado é do último commit dele); bits 3–7 reservados |
| 4 | 4 | `commit_seq` | 32 bits baixos do tick do grupo dono em que o resultado foi produzido |
| 8 | 8 | `applied` | valor aplicado nesse commit |
| 16 | 8 | `mine` | a proposta ativa deste escritor (`mine()`, 17 §11), válida com `MINE_ACTIVE` |

**Secção larga:** os valores de `string`, `bytes` e `Internal<T>` (≤ 64 B cada, ref/00 §10), cada um num offset múltiplo de 8. `string` em UTF-8, `bytes` crus, registos em CBOR (§4.10).

O `image-len` fica fixo durante a geração: é `64 + 24 × (n_slots + n_outcomes) + 64 × n_largos`, arredondado a 8.

### 3.3 O buffer de propostas

```
buffer := cabeçalho (8 B) · registo*
```

| Offset | Tamanho | Campo |
|---|---|---|
| 0 | 1 | `abi` (1) |
| 1 | 1 | reservado |
| 2 | 2 | `n_records` |
| 4 | 4 | `len`: comprimento total, com o cabeçalho |

**Registos** (empacotados, sem preenchimento; o primeiro byte é o tipo):

| Tipo | Nome | Disposição | Tamanho |
|---|---|---|---|
| 1 | `WRITE` | `kind u8 · flags u8 · port u32 · value u64` [· `data[L]`] [· `at u64`] | 14 B (+ L) (+ 8) |
| 2 | `METHOD` | `kind u8 · flags u8 (=0) · instance u32 · method u16 · len u16 · args[len]` | 10 B + len |
| 3 | `PUBLISH` | `kind u8 · flags u8 · port u32 · value u64` [· `data[L]`] | 14 B (+ L) |
| 4 | `COUNTER_ADD` | `kind u8 · flags u8 (=0) · port u32 · delta u64` | 14 B |
| 5 | `EMIT` | `kind u8 · flags u8 (=0) · port u32 · len u16 · payload[len]` | 8 B + len |
| 6 | `NOTE` | `kind u8 · flags u8 (=0) · port u32` (leitura substituída) | 6 B |

Flags de `WRITE` e `PUBLISH`: bit 0 `RELINQUISH` (só `WRITE`; `value` = 0); bit 1 `INLINE` (`value` = comprimento L ≤ 64, e seguem-se L bytes: `string`, `bytes`, ou CBOR de um registo); bit 2 `AT` (só `WRITE`, **1.1**, `scheduled-writes`: segue-se `at_mono_ns`). Bits 3–7 reservados.

Limites: `args` ≤ 4 KiB; `payload` ≤ 8 KiB; capacidade do buffer por omissão 16 KiB (`#[joker::app(proposal_buffer = "…")]`, máximo 1 MiB).

### 3.4 Ordem, validação e erros

1. **Ordem.** Numa chamada a `on-cycle`, os registos feitos por funções de `cycle-io` (acesso dinâmico) vêm primeiro, pela ordem das chamadas; os do buffer devolvido vêm a seguir, pela ordem do buffer. Os SDKs (Rust e tier 2) escrevem só no buffer.
2. **Árbitro.** Várias propostas do mesmo escritor na mesma saída e no mesmo tick: vence a última (16, A4). As propostas async drenadas no passo 2 são anteriores a todas as de ciclo.
3. **Métodos.** Os registos `METHOD` são expandidos no passo 5a, pela ordem das chamadas (17 §4.1), com `own()` a ver as propostas anteriores do mesmo tick.
4. **Validação estrutural** pelo host, antes de aplicar qualquer registo: `abi`, `len` e `n_records` coerentes com o tamanho da lista; tipo conhecido; flags reservadas a 0; `port` dentro da tabela e com o tipo de porta certo (`WRITE` → saída ou membro `out` ou comando virtual; `PUBLISH` → publicado, alarme, `Internal` de escritor de ciclo, membro `state`; `COUNTER_ADD` → `Counter`; `EMIT` → evento de saída ou evento de classe; `NOTE` → porta com amostra); `instance` e `method` dentro das tabelas; comprimentos dentro dos limites. Uma falha estrutural **descarta o buffer inteiro** e é um trap (`J-E0907`): conta para o crashloop como qualquer outro trap (01 §9).
5. **Validação de valor**, registo a registo: `bool` só 0 ou 1; `enum` só variantes declaradas; `f64` NaN ou infinito; `string` UTF-8 inválido. Dá `rejected(wrong-type)` a essa proposta, no passo 1 do envelope, e não é trap.
6. **Buffer cheio.** O SDK não escreve para lá da capacidade: as propostas seguintes são descartadas, e a tarefa termina com `cycle-result.error = J-E0203`. As que já estão no buffer seguem.

### 3.5 Custo

Com 60 entradas e 12 saídas (grupo de 1 ms do documento 09, ataque 2): 64 + 24 × 72 + 24 × 12 = 2 080 B de imagem, copiados numa operação pelo ABI canónico para um buffer pré-reservado do guest. Uma app de classes com 720 portas (documento 14, A3) tem ~20 KB por tick. O resultado por saída (24 B) é o que permite `last_outcome()`, `mine()` e `winner()` sem chamadas ao host.

---

## 4. O SDK Rust `joker-sdk`

### 4.1 Crate, dependência e backends

```toml
# apps/peak_shaver/Cargo.toml (gerado por `jk app new`)
[package]
name = "peak_shaver"
version = "1.4.2"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
joker = { package = "joker-sdk", version = "0.1" }

[build-dependencies]
joker-build = "0.1"

[dev-dependencies]
joker-test = "0.1"
```

```rust
// apps/peak_shaver/build.rs (gerado)
fn main() {
    joker_build::generate(); // classes locais (classes/local/*.jkc) → OUT_DIR/joker_classes.rs
}
```

| Crate | Papel | `no_std` |
|---|---|---|
| `joker-sdk` | a API das apps; `use joker::prelude::*` | sim; `alloc` só no backend Wasm e em Linux nativo |
| `joker-sdk-macros` | as macros; versão igual à do `joker-sdk` (dependência `=`) | — (proc-macro) |
| `joker-classes-std` | handles gerados pelo `jkc` para `classes/std/`, reexportados em `joker::classes::std` | sim |
| `joker-build` | gera as classes locais em `build.rs` | — |
| `joker-test` | o harness (§6) | não |

**Backends**, escolhidos pelo alvo, sem código diferente na app. São duas features Cargo exclusivas do `joker-sdk`: `backend-wasm` (a de omissão) e `backend-native`; o `jk build` escolhe a certa.

| Backend | Feature | Alvo | Quem o escolhe | Contrato com o host | Metadados |
|---|---|---|---|---|---|
| `wasm` | `backend-wasm` | `wasm32-wasip2` (componente) | `jk build` | os exports do mundo (§2.6) | secção `joker:meta` (§5.1) |
| `native-linux` | `backend-native` | o do `jokerd` (`kind = "native"`, 04 §5) | `jk runtime build --features native-apps` | o trait `NativeApp` (ref/08 §2.11.5) | secção ELF `.joker_meta` |
| `native-micro` | `backend-native`, com `--cfg joker_micro` | `thumbv8m.main-none-eabihf`, `riscv32imac-…` (19 §5) | `jk build --target mcu:…` | `NativeApp`, avançado pelo `joker-micro` | secção ELF `.joker_meta` |

Com `backend-native`, a macro `#[joker::app]` gera `impl NativeApp` em vez dos exports WIT. O `NativeApp` recebe a mesma imagem (§3.2) e devolve o mesmo buffer de propostas (§3.3), e as tarefas async são futuros de tamanho fixo avançados pelo host (no MCU, por `jk_service`).

**Versões:** `joker-sdk 0.y` gera `joker:runtime@0.y.0`; a partir da 1.0, `joker-sdk 1.y` gera `joker:runtime@1.y.0`. Para correr em runtimes mais antigas, fixa-se uma minor mais antiga do SDK. O `jk` avisa quando o mundo gerado é mais novo do que o `[project] joker` do manifesto.

**Registo das tarefas.** As tarefas (`#[joker::cycle]`, `#[joker::task]`, `#[joker::on_action]`) não são listadas no `#[joker::app]`: cada macro regista-se em tempo de ligação (construtores em `.init_array`, corridos por `__wasm_call_ctors` no Wasm e pelo arranque gerado pelo `jk build` no Micro) e emite o seu fragmento de `joker:meta` (§5.2). A ordem das tarefas é a do nome, nos dois lados.

### 4.2 Macros de app e de porta

#### `#[joker::app(…)]` (na struct da app, na raiz do crate)

| Argumento | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| `name` | string | não | `CARGO_PKG_NAME` com `-` → `_` | `[a-z][a-z0-9_]{0,31}`, sem prefixo `joker` (`J-E0710`) |
| `version` | string SemVer | não | `CARGO_PKG_VERSION` | SemVer (`J-E0710`) |
| `requires_clock` | `"holdover"` \| `"synced"` | não | nenhum | a mais estrita entre esta e a do manifesto (ref/01 `requires_clock`) vale |
| `secrets` | lista de nomes | não | `[]` | nomes locais `snake_case`; ligados em `[apps.<app>.secrets]` (ref/02) |
| `optional_secrets` | lista de nomes | não | `[]` | idem, sem erro se não ligados |
| `proposal_buffer` | tamanho (`"16 KiB"`) | não | `"16 KiB"` | 1 KiB – 1 MiB |

A doc comment da struct vai para `joker:meta` (chave 17). A macro gera: `impl joker::App for <App>`, as vistas `CycleIo<'_, App>` e `TaskIo<'_, App>` (§4.5), o módulo `classes` com as classes locais (se `build.rs` as gerou), os exports do mundo e o fragmento de metadados. Uma segunda `#[joker::app]` no crate, ou uma fora da raiz, é `J-E0711`. Os itens gerados têm a visibilidade da struct.

Os campos da struct têm um tipo de declaração (§4.4) e **um** atributo de porta:

| Atributo | Tipo do campo | Argumentos (obrigatórios **a negrito**) | Omissões e validação |
|---|---|---|---|
| `#[input]` | `Input<T>`, `InputMany<T>` (com `many`), `EventInput<E>` (com `event`) | `unit`, `quantity`, `optional`, `many`, `min`, `max`, `event` | numérico sem `unit` `J-E0703` (use `unit = "1"`); unidade fora do catálogo `J0109`; `many`: `min` 1, `max` 256 |
| `#[output]` | `Output<T>`, `OutputMany<T>`, `EventOutput<E>` | `unit`, `quantity`, `optional`, `many`, `min`, `max`, `event`, `group` | como `#[input]`; `group`: grupo dono pedido (16, A11) |
| `#[param]` | `Param<T>` | `unit`, `quantity`, `range`, `default` | `range = "a..b"` é **fechado** nos dois lados (`"a..=b"` é igual; `"..b"` e `"a.."` abertos); `default` fora da gama `J-E0706`; sem `default`, o manifesto tem de dar um valor (`J0142`, ref/01) |
| `#[internal]` | `Internal<T: Record>` | `writer = "async" \| "cycle"` | omissão `"async"`; registo > 64 B `J-E0708` |
| `#[publish]` | `Published<T>`, `Counter` | `unit`, `quantity` | caminho `app.<app>.<campo>` |
| `#[alarm]` | `Alarm` | **`severity`**, **`message`**, `delay_on`, `delay_off` | alarme com o id `app.<app>.<campo>` (o que a API e a `jk` mostram, ref/09 §5.8) e os sinais `alarm.app.<app>.<campo>.state` e `.active` (02 §11) |
| `#[action]` | `Action<A, R>` | `long` | `A` e `R`: `()` ou `Record`; ligado a uma ação de dispositivo (`"pcs.reset_fault"`) |
| `#[mailbox]` | `Mailbox<T: Record, N>` | **`to = "cycle" \| "async"`** | 1 ≤ N ≤ 64; mensagem ≤ 256 B |
| `#[device]` | tipo de classe, `Many<C, MIN, MAX>`, `Option<C>` | `requires = [...]` | `Option<C>`: dispositivo opcional; 0 ≤ MIN ≤ MAX ≤ 1024 (`J-E0717`) |
| `#[model]` | `Model` | **`inputs`**, **`outputs`** | **1.1**, feature `unstable-ml` (`J-E0718` sem ela) |

Tipos `T` admitidos em portas escalares: `bool`, `i64`, `u64`, `f64`, `f32`, enums com `#[derive(joker::Enum)]` (ou gerados pelo `jkc`), `Bits<N>`, `BoundedString<N>` e `BoundedBytes<N>` com N ≤ 64. Qualquer outro (`i32`, `u8`, `String`) é `J-E0707`, com a sugestão do tipo certo.

Nomes de campos: `snake_case` único na app. São reservados (`J-E0709`) os nomes de portas da ref/00 §2 (`now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta`), o prefixo `joker` e os nomes começados por `_`; numa app que implementa uma classe, também `events`. São os mesmos em todas as linguagens (ref/05 §5.5).

#### `#[joker::implements(Classe, capabilities = [...])]`

Na mesma struct da app, antes ou depois de `#[joker::app]` (a ordem é indiferente). `Classe` é um tipo de classe (`Pump`, `StatusLight`); `capabilities` são as capacidades que a implementação oferece (omissão: nenhuma). Gera `io.cmd`, `io.state` e `io.events` (§4.8.4). Classe desconhecida ou capacidade que a classe não tem: `J-E0715`.

#### `#[joker::state(persist, version = N)]`

No tipo de estado de uma tarefa. `persist` faz o estado sobreviver à troca a quente (§4.12); `version` (omissão 1) é a versão do esquema do estado. Um campo que não se codifica (§4.10) é `J-E0720`. `#[joker::migrate(<Tipo>)]` numa função dá a migração à medida desse tipo (§4.12).

#### Derives

| Derive | Em | Gera | Regras |
|---|---|---|---|
| `#[derive(joker::Record)]` | struct com campos nomeados | `impl Record`, esquema em `joker:meta`, tamanho máximo codificado | campos: escalares, `Instant`, `MonoNanos`, `UtcNanos`, `UtcSeconds`, `Duration`, `Quality`, enums, `Option<T>`, outros `Record`, `BoundedVec<T, N ≤ 64>`, `BoundedString<N ≤ 1024>` (`J-E0721`) |
| `#[derive(joker::Event)]` | idem | `Record` + marca de evento | idem |
| `#[derive(joker::Enum)]` | enum sem dados, variantes com valor explícito ou implícito (0, 1, …) | `impl Enum` (u64), `Copy`, `Eq`, esquema | valores únicos |
| `#[derive(joker::Class)]` | struct com campos `#[input]`/`#[output]` | classe local (§4.8.5) | só `Input<T>` e `Output<T>` |

Os derives e os traits com o mesmo nome coexistem (como no `serde`): `joker::Record` é as duas coisas.

### 4.3 Macros de tarefas

#### 4.3.1 `#[joker::cycle(…)]`

| Argumento | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| `period` | duração | **sim** | — | `J-E0714`; é um pedido (04 §3.2, `J0130`) |
| `wcet` | duração | não | medido por `jk bench` | ≤ `period` (`J-E0714`) |
| `init` | caminho de `fn(&CycleIo<A>) -> S` | não | `S::default()` | |
| `optional` | bool | não | `false` | desativada por `overrun = "degrade"` (01 §4) |

Assinaturas aceites (outras: `J-E0712`, com a assinatura esperada):

```rust
#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<PeakShaver>, st: &mut Ctl) -> Result<()>   // S: Default, ou init = …
#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<PeakShaver>) -> Result<()>                 // sem estado
```

#### 4.3.2 `#[joker::task(…)]`

Exatamente um gatilho (`J-E0713` com zero ou dois):

| Gatilho | Forma | Argumento extra da função | Semântica |
|---|---|---|---|
| `every` | `every = "15m"`, `jitter = "30s"` (opcional) | — | periódico; `jitter` uniforme a partir do gerador determinista |
| `on` | `on = "soc"` ou `on = ["a", "b"]`; membros com `"<dispositivo>.<membro>"` | `s: Sample<T>` (uma porta) ou `c: Change` (várias) | a cada `seq` nova; mudanças que chegam durante uma execução juntam-se na última |
| `on_event` | `on_event = "detections"` ou `"<dispositivo>.<evento>"` | `e: E` ou `d: Delivery<E>` | cada evento é entregue, por ordem, um de cada vez; fila de 64, perdas em `Delivery::lost` |
| `at` | `at = "cron: 0 */15 * * * *"` | — | cron de 6 campos, UTC; exige relógio `synced` ou `holdover` (sem ele, o disparo espera) |
| `startup` | `startup` | — | uma vez, depois do `init`; os outros gatilhos da app só são entregues depois de todas as tarefas `startup` terminarem |

```rust
#[joker::task(every = "15m", jitter = "30s")]
async fn replan(io: &TaskIo<PeakShaver>) -> Result<()>
#[joker::task(on = "soc")]
async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()>
#[joker::task(on_event = "detections")]
async fn decide(io: &TaskIo<Inspector>, d: Detections) -> Result<()>
#[joker::task(every = "1h")]
async fn tally(io: &TaskIo<Inspector>, st: &Shared<Totals>) -> Result<()> // estado entre execuções
```

- Uma tarefa nunca corre em paralelo consigo mesma; tarefas diferentes da mesma app intercalam-se nos `await` (a instância async tem uma só thread). Um gatilho que dispara durante uma execução fica pendente (um só); os que se perdem contam em `joker.app.<app>.task.<t>.coalesced`.
- `Shared<S>` (`S: Default`) é o estado de uma tarefa async entre execuções. Só se acede com `st.with(|s| …)` e `st.with_mut(|s| …)`, de modo que nunca atravessa um `await`.

#### 4.3.3 `#[joker::on_action(<ação>)]`

Numa app com `#[joker::implements]`. Serve uma `action` da classe implementada.

```rust
#[joker::on_action(reset)]
async fn reset(io: &TaskIo<PumpCtl>, args: (), cx: &ActionCx) -> Result<(), ActionError>
```

`args` e o tipo de sucesso são os gerados pelo `jkc` para a ação; `cx` é opcional. `ActionCx`: `key() -> &str`, `caller() -> &str`, `deadline() -> MonoNanos`, `progress(fraction: Option<f32>, message: &str)`, `is_cancelled() -> bool`. Uma ação da classe (numa capacidade oferecida) sem handler é `J-E0716` no `jk build`.

#### 4.3.4 `#[joker::on_params]`

```rust
#[joker::on_params]
fn check(changes: &ParamChanges) -> Result<()>
```

Opcional, um por app. `ParamChanges`: `iter() -> impl Iterator<Item = (&str, Value)>`, `get<T: PortValue>(&self, name: &str) -> Option<T>`. Serve para recusar combinações inválidas (`min > max`): o `Err` recusa a geração para esta app (`J-E0011`). Os valores novos chegam à instância de ciclo na imagem do tick seguinte ao limite de troca, e à async em `param()`.

### 4.4 Tipos de declaração e o que dão em cada vista

Os campos da struct da app são **tipos de declaração** (tamanho zero): a app nunca os constrói. O `#[joker::app]` dá a cada campo, em cada vista, o tipo de handle certo:

| Declaração | Em `CycleIo` | Em `TaskIo` |
|---|---|---|
| `Input<T>` | `CycleInput<'_, T>` | `TaskInput<'_, T>` |
| `InputMany<T>` | `CycleInputMany<'_, T>` | `TaskInputMany<'_, T>` |
| `Output<T>` | `CycleOutput<'_, T>` | `TaskOutput<'_, T>` |
| `OutputMany<T>` | `CycleOutputMany<'_, T>` | `TaskOutputMany<'_, T>` |
| `Param<T>` | `ParamHandle<'_, T>` | `ParamHandle<'_, T>` |
| `Internal<T>` | `CycleInternal<'_, T>` (`set` só com `writer = "cycle"`) | `TaskInternal<'_, T>` (`set` só com `writer = "async"`) |
| `Published<T>` | `PublishedHandle<'_, T>` | idem |
| `Counter` | `CounterHandle<'_>` | idem |
| `Alarm` | `AlarmHandle<'_>` | idem |
| `EventOutput<E>` | `EventOutputHandle<'_, E>` | idem |
| `EventInput<E>` | indisponível (`J-E0701`) | `EventInputHandle<'_, E>` |
| `Action<A, R>` | indisponível (`J-E0701`) | `ActionPort<'_, A, R>` |
| `Mailbox<T, N>` | `MailboxEnd<'_, T>` (lado do ciclo) | `MailboxEnd<'_, T>` (lado async) |
| classe `C` | `<C>Cycle<'_>` | `<C>Task<'_>` |
| `Many<C, MIN, MAX>` | `ManyHandle<'_, <C>Cycle<'_>, MAX>` | `ManyHandle<'_, <C>Task<'_>, MAX>` |
| `Option<C>` | `Option<<C>Cycle<'_>>` | `Option<<C>Task<'_>>` |
| `Model` | indisponível (`J-E0701`) | `ModelHandle<'_>` (1.1) |

Usar numa vista uma operação que só existe na outra dá um erro de compilação com código e explicação, e não um "método não encontrado": os métodos existem nas duas vistas, com um limite de trait que só a certa cumpre (`#[diagnostic::on_unimplemented]`, estável desde o Rust 1.78):

```
error[E0277]: J-E0701: `write_at` is only available in async tasks (TaskIo)
  --> apps/inspector/src/lib.rs:21:21
   |
21 |         io.ejector.write_at(true, t);
   |                    ^^^^^^^^ cycle tasks cannot wait for a commit
   = note: a cycle task that needs this publishes an event; an async task acts on it (doc 02 §8)
```

### 4.5 As vistas `CycleIo` e `TaskIo`

```rust
pub trait App: 'static {
    type Cycle<'a>;
    type Task<'a>;
    // gerado por #[joker::app]; não se implementa à mão
}

pub struct CycleIo<'a, A: App> { /* … */ }
impl<'a, A: App> core::ops::Deref for CycleIo<'a, A> { type Target = A::Cycle<'a>; }
impl<'a, A: App> CycleIo<'a, A> {
    pub fn now(&self) -> Instant;               // instante do latch
    pub fn dt(&self) -> Duration;               // período efetivo do grupo
    pub fn tick(&self) -> u64;                  // tick_seq
    pub fn first_tick(&self) -> bool;           // FIRST_TICK
    pub fn after_skip(&self) -> bool;           // AFTER_SKIP
    pub fn shadow(&self) -> bool;               // a app corre em sombra (M16)
    pub fn task(&self) -> &'static str;
    pub fn random(&self) -> u64;
}

pub struct TaskIo<'a, A: App> { /* … */ }
impl<'a, A: App> core::ops::Deref for TaskIo<'a, A> { type Target = A::Task<'a>; }
impl<'a, A: App> TaskIo<'a, A> {
    pub fn now(&self) -> Instant;
    pub async fn sleep(&self, d: Duration);
    pub async fn sleep_until(&self, t: MonoNanos);
    pub fn snapshot<S: Snapshot>(&self, ports: S) -> Result<S::Samples>; // tuplos até 12: (&io.a, &io.b)
    pub fn random(&self) -> u64;
    pub fn kv(&self) -> Kv<'_>;
    pub fn secret(&self, name: &str) -> Result<Secret>;                  // alloc
    pub fn shadow(&self) -> bool;
}
```

A vista de ciclo recebe-se como `&mut CycleIo<A>`; a async como `&TaskIo<A>`, porque várias tarefas async da mesma app a partilham. Todas as operações de proposta recebem `&self` (o buffer é interior), o que evita conflitos de empréstimo entre handles.

### 4.6 Handles de porta

Corpos omitidos. `T: PortValue` (§4.7). Os valores estão sempre na unidade declarada da porta.

```rust
// Entradas: iguais nas duas vistas, salvo onde marcado.
impl<'a, T: PortValue> CycleInput<'a, T> {
    pub fn get(&self) -> Result<T, QualityError>;            // Good → Ok; senão Err (16 §3.1)
    pub fn get_uncertain(&self) -> Result<T, QualityError>;  // Good, Uncertain → Ok; Bad → Err
    pub fn get_or(&self, default: T) -> T;                   // não Good → default, e NOTE no traço
    pub fn sample(&self) -> Sample<T>;
    pub fn is_bound(&self) -> bool;
    pub fn changed(&self) -> bool;                           // CHANGED
}
impl<'a, T: PortValue> TaskInput<'a, T> {
    // get, get_uncertain, get_or, sample, is_bound: como em CycleInput, sobre o slot atual
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>;   // média por passo
    pub async fn history_agg(&self, from: Instant, to: Instant, step: Duration, agg: Agg) -> Result<History<T>>;
    pub fn subscribe(&self) -> Result<Changes<'a, T>>;       // Stream<Item = Sample<T>>
}

// Saídas.
impl<'a, T: PortValue> CycleOutput<'a, T> {
    pub fn set(&self, v: T);                                 // proposta (renova o lease, 16 A2)
    pub fn relinquish(&self);
    pub fn pending(&self) -> Option<T>;
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>>;
    pub fn get(&self) -> Result<T, QualityError>;            // valor aplicado
    pub fn sample(&self) -> Sample<T>;
    pub fn mine(&self) -> Option<T>;                         // proposta ativa deste escritor
    pub fn winner(&self) -> Option<Level>;                   // nível que ganhou; None = safe
    pub fn is_bound(&self) -> bool;
}
impl<'a, T: PortValue> TaskOutput<'a, T> {
    pub fn set(&self, v: T);                                 // sem esperar (deferred)
    pub fn write(&self, v: T) -> WriteTicket<'a, T>;         // .await → Result<WriteOutcome<T>>
    pub fn relinquish(&self);
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>>;
    pub fn get(&self) -> Result<T, QualityError>;
    pub fn sample(&self) -> Sample<T>;
    pub fn mine(&self) -> Option<T>;
    pub fn winner(&self) -> Option<Level>;
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>;
    #[cfg(feature = "unstable-scheduled-writes")]
    pub fn write_at(&self, v: T, at: impl Into<MonoNanos>) -> WriteTicket<'a, T>;
    #[cfg(feature = "unstable-intents")]
    pub async fn propose(&self, v: T, reason: &str, ttl: Duration) -> Result<IntentId>;
}
pub struct WriteTicket<'a, T> { /* … */ }
impl<'a, T: PortValue> core::future::IntoFuture for WriteTicket<'a, T> { type Output = Result<WriteOutcome<T>>; }

// Portas múltiplas: ordem = a do bind (lista) ou lexicográfica (seletor); tamanho fixo na geração.
impl<'a, T: PortValue> CycleInputMany<'a, T> {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, i: usize) -> Option<&CycleInput<'a, T>>;
    pub fn iter(&self) -> core::slice::Iter<'_, CycleInput<'a, T>>;
    pub fn names(&self) -> impl Iterator<Item = &str>;       // caminhos resolvidos
}
// CycleOutputMany, TaskInputMany, TaskOutputMany: len, is_empty, get, iter, iter_mut, names.

impl<'a, T: PortValue> ParamHandle<'a, T> {
    pub fn get(&self) -> T;                                  // um parâmetro tem sempre valor
}

impl<'a, T: Record> CycleInternal<'a, T> {                   // e TaskInternal
    pub fn get(&self) -> Result<T, QualityError>;            // Uncertain(Initial) até à primeira escrita
    pub fn get_or(&self, default: T) -> T;
    pub fn get_or_default(&self) -> T where T: Default;
    pub fn sample(&self) -> Sample<T>;
    pub fn set(&self, v: T);                                 // só na vista do escritor declarado
}

impl<'a, T: PortValue> PublishedHandle<'a, T> { pub fn set(&self, v: T); }
impl<'a> CounterHandle<'a> { pub fn inc(&self); pub fn add(&self, n: u64); }
impl<'a> AlarmHandle<'a> {
    pub fn raise(&self);
    pub fn clear(&self);
    pub fn set(&self, active: bool);
    pub fn state(&self) -> AlarmState;                       // alarm.app.<app>.<nome>.state, do latch ou do slot
}
impl<'a, E: Event> EventOutputHandle<'a, E> { pub fn emit(&self, e: E); }
impl<'a, E: Event> EventInputHandle<'a, E> {
    pub async fn next(&self) -> Result<Delivery<E>>;
    pub fn stream(&self) -> Result<Deliveries<'a, E>>;       // Stream<Item = Result<Delivery<E>>>
}
impl<'a, T: Record> MailboxEnd<'a, T> {
    pub fn try_send(&self, v: T) -> Result<(), Full<T>>;     // só no lado emissor (mailbox-send)
    pub fn lost(&self) -> u64;                               // só no lado emissor: envios recusados por fila cheia
    pub fn try_recv(&self) -> Option<T>;                     // só no lado recetor (mailbox-recv)
    pub fn drain(&self) -> impl Iterator<Item = T> + '_;     // só no lado recetor: try_recv até vazia
    pub async fn recv(&self) -> T;                           // só no lado recetor, em TaskIo (mailbox-next)
}
```

`Delivery<E> { pub event: E, pub seq: u64, pub at: Instant, pub lost: u32 }`. `History<T>` desreferencia para `[Sample<T>]`. Um `Alarm` e um `Published<T>` podem ser escritos das duas vistas: o grupo dono é o da primeira tarefa de ciclo da app (ou `default`), e numa escrita das duas no mesmo tick vence a de ciclo (16, A4).

#### 4.6.1 Outros tipos da API async

```rust
pub enum Agg { Raw, Mean, Min, Max, First, Last, Count }       // history_agg; history() usa Mean
pub struct Change { pub port: &'static str, pub sample: Sample<Value> }   // on = ["a", "b"]
pub struct Full<T>(pub T);                                      // mailbox cheia: devolve a mensagem
pub enum AlarmState { Normal, ActiveUnacked, ActiveAcked, RtnUnacked, Unknown }   // ref/00 §6
pub trait Snapshot { type Samples; }                            // tuplos (&h1, …, &h12) de handles com amostra

impl<'a> Kv<'a> {
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub fn get_record<T: Record>(&self, key: &str) -> Result<Option<T>>;
    pub async fn set(&self, key: &str, value: &[u8]) -> Result<()>;
    pub async fn set_record<T: Record>(&self, key: &str, value: &T) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<()>;
    pub fn keys(&self, prefix: &str) -> Result<Vec<String>>;
    pub fn batch(&self) -> KvBatch<'a>;                         // .set(k, v).delete(k).commit().await
}

pub struct Secret { /* bytes, apagados no drop; sem Debug nem Display do valor */ }
impl Secret { pub fn expose(&self) -> &[u8]; pub fn name(&self) -> &str; }
```

`Changes<'a, T>` e `Deliveries<'a, E>` implementam `futures_core::Stream`. `IntentId` (1.1) é um ULID em `BoundedString<26>`.

### 4.7 Tipos de valor, qualidade e tempo

```rust
pub trait PortValue: Copy + PartialEq + core::fmt::Debug + 'static { /* selado */ }
// bool, i64, u64, f64, f32, Bits<N>, BoundedString<N ≤ 64>, BoundedBytes<N ≤ 64>, T: Enum

pub struct Sample<T> {
    pub value: T,                  // T::default() se nunca houve valor (Uncertain(Initial))
    pub quality: Quality,
    pub ts_src: Option<UtcNanos>,
    pub ts_hw: Option<MonoNanos>,
    pub ts_acq: MonoNanos,
    pub seq: u64,
}
impl<T> Sample<T> {
    pub fn good(&self) -> Option<&T>;
    pub fn age(&self, now: Instant) -> Duration;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quality { pub code: QualityCode, pub reason: QualityReason }
impl Quality {
    pub const GOOD: Quality;
    pub const fn good() -> Self;
    pub const fn uncertain(r: QualityReason) -> Self;
    pub const fn bad(r: QualityReason) -> Self;
    pub fn is_good(&self) -> bool;
    pub fn is_uncertain(&self) -> bool;
    pub fn is_bad(&self) -> bool;
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityCode { Good = 0, Uncertain = 1, Bad = 2 }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum QualityReason {
    None = 0, Overridden = 1, LocalOverride = 2, Stale = 3, NotConfirmed = 4, Substituted = 5,
    OutOfDistribution = 6, SensorCal = 7, ClockUnsynced = 8, Initial = 9, Frozen = 10,
    PartialWrite = 11, CommFailure = 12, DeviceFault = 13, OutOfRange = 14, ConfigError = 15,
    NotConnected = 16, Nan = 17, Tripped = 18, Deadline = 19,
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub enum WriteOutcome<T> {
    Accepted,
    Clamped { by: ClampKind, applied: T },
    Suppressed,
    Overridden { level: Level },
    Deferred,
    Shadowed,
    Rejected(RejectReason),
    Tripped,
}
impl<T: PortValue> WriteOutcome<T> {
    pub fn code(&self) -> u8;                                  // ref/00 §4.4
    pub fn is_applied(&self) -> bool;                          // Accepted | Clamped
    pub fn require_applied(self) -> Result<Self>;              // senão Err(J-E0204)
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ClampKind { Range, Rate }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RejectReason { WrongType = 1, Interlock, Quality, Range, AtomicGroup, NotOwner, ClockUnsynced, Budget }

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Level(core::num::NonZeroU8);                        // 1–16
impl Level {
    pub fn get(self) -> u8;
    pub fn band(self) -> Band;                                 // Safety, Manual, Supervisor, Control, Optimizer, Schedule
}
// Display: "manual:3"

pub use core::time::Duration;
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct MonoNanos(pub u64);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct UtcNanos(pub i64);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct UtcSeconds(pub i64);
impl UtcSeconds { pub const MIN: Self; pub const MAX: Self; }
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ClockQuality { Synced, Holdover, Unsynced }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Instant { pub mono: MonoNanos, pub utc: UtcNanos, pub clock: ClockQuality }
impl Instant {
    pub fn utc_seconds(&self) -> UtcSeconds;
}
// Instant ± Duration → Instant (os dois relógios); Instant − Instant → Duration (monotónico, saturado)
// MonoNanos ± Duration → MonoNanos; UtcSeconds ± i64 → UtcSeconds; Instant: Into<MonoNanos>

pub trait DurationExt {
    fn ns(self) -> Duration;
    fn us(self) -> Duration;
    fn ms(self) -> Duration;
    fn s(self) -> Duration;
    fn mins(self) -> Duration;
    fn h(self) -> Duration;
    fn d(self) -> Duration;
}
impl DurationExt for u64 { /* … */ }
impl DurationExt for f64 { /* … */ }        // negativo ou NaN → Duration::ZERO; acima do máximo → Duration::MAX
```

- `200.ms()` e `io.travel.get().ms()` funcionam: o trait está implementado para **um só** tipo inteiro (`u64`), e o literal infere-o. O minuto é **`mins()`**, e não `min()`, que colidiria com `Ord::min` (erro E0034) e com `f64::min`.
- Tipos limitados, sem alocação: `BoundedVec<T, const N: usize>` (`push -> Result<(), T>`, `iter`, `len`, `as_slice`, `Deref<[T]>`), `BoundedString<const N: usize>` (`as_str`, `try_from(&str)`), `BoundedBytes<const N: usize>`, `Bits<const N: usize>` (`bit(n) -> bool`, `bits() -> u64`).
- `Value`: `enum Value { Bool(bool), I64(i64), U64(u64), F64(f64), Text(BoundedString<64>), Bytes(BoundedBytes<64>) }`, com `From` de todos os `PortValue` e `PartialEq<f64>`, `PartialEq<bool>`, `PartialEq<i64>`, `PartialEq<u64>`.

#### 4.7.1 Erros

```rust
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Clone, PartialEq, Eq)]
pub struct Error { /* code: Code, message: BoundedString<256> */ }
impl Error {
    pub fn new(code: Code, message: impl core::fmt::Display) -> Self;   // mensagem truncada a 256 B
    pub fn app(message: impl core::fmt::Display) -> Self;               // J-E0050
    pub fn quality(members: MemberSet) -> Self;                         // J-E0501
    pub fn code(&self) -> Code;
    pub fn message(&self) -> &str;
}
impl core::fmt::Debug for Error { /* "J-E0303: action timed out after 5 s" */ }
impl core::fmt::Display for Error { /* idem */ }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Code(/* [u8; 8], ASCII */);
impl Code {
    pub fn as_str(&self) -> &str;
    pub const fn from_static(s: &'static str) -> Self;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QualityError { pub port: &'static str, pub quality: Quality }   // J-E0101, J-E0102 ou J-E0103

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActionError { pub kind: ActionErrorKind, pub error: Error }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ActionErrorKind { Denied, Forbidden, Timeout, Device, Invalid, Busy, NotConnected, Cancelled, Conflict, Unsupported, Failed, Other }
impl ActionError {
    pub fn device(message: impl core::fmt::Display) -> Self;    // J-E0304
    pub fn invalid(message: impl core::fmt::Display) -> Self;   // J-E0305
    pub fn cancelled() -> Self;                                 // J-E0308: um on_action que viu is_cancelled()
}

// From<QualityError>, From<ActionError> (e From<InferError> com unstable-ml) para Error: o `?` funciona.
```

Um código do host que o SDK não conhece passa intacto em `Code`: a app pode compará-lo (`e.code().as_str() == "J-E0303"`) sem que o SDK tenha de ser atualizado. `ActionErrorKind` é derivado do código (tabela do §7). `Cancelled` cobre as duas pontas de uma ação longa cancelada: quem chamou recebe `J-E0308`, e quem a serve em `on_action` devolve `ActionError::cancelled()` depois de ver `cx.is_cancelled()` (a ABI C tem o par `JK_ACTION_CANCELLED`, ref/08 §2.6.j).

### 4.8 Classes

#### 4.8.1 Nomes gerados

O `jkc` (documento 17, §6) gera, para cada classe, um módulo Rust em `joker::classes::<pacote>` (classes do registo) ou em `crate::classes::local` (classes do projeto, via `build.rs`):

| Da classe | Rust | Exemplo |
|---|---|---|
| nome da classe | UpperCamelCase do nome, sem pontos | `pump` → `Pump`; `temp_sensor` → `TempSensor`; `meter.ac3` → `MeterAc3`; `hvac.vav` → `HvacVav` |
| handles | `<Nome>Cycle<'a>`, `<Nome>Task<'a>` | `LedCycle<'a>` |
| estado de `read()` | `<Nome>State` | `PumpState` |
| enums, registos | UpperCamelCase; variantes em UpperCamelCase | `StatusMode::Warn` |
| comandos, estado e eventos de uma implementação | `<Nome>Cmd<'a>`, `<Nome>StateOut<'a>`, `<Nome>Events<'a>` | `PumpCmd<'a>` |
| majors | `joker::classes::std::v<major>::<Nome>`; o nome sem módulo aponta para a major que o crate tinha quando a classe apareceu, e nunca muda dentro da major do crate | `joker::classes::std::v2::InverterPv` |

Pode-se importar com outro nome (`use joker::classes::std::HvacVav as Vav;`).

#### 4.8.2 Handles

Para uma classe com membros `in x`, `out y`, `param z`, `query q`, métodos `m(args)`, eventos `e` e ações `a(args) -> R`:

| Operação | `<C>Cycle` | `<C>Task` |
|---|---|---|
| `x()`, `y()`, `z()`, `q()` | `Result<T>`: o membro com qualidade `Good` (um `out` dá o valor aplicado, 17 §11); senão `Err` (`J-E0101`); membro sem ligação `J-E0506`; capacidade em falta `J-E0502` | idem, sobre o slot |
| `m(args)` | `()`: acrescenta um registo `METHOD` | `MethodCall` → `.await` dá `Result<MethodOutcome>` |
| `a(args)` | indisponível (`J-E0701`) | `ActionCall<'_, R>` (§4.9) |
| eventos | indisponível | `on_event = "<dispositivo>.<e>"`, ou `events_e() -> Result<Deliveries<'_, E>>` |
| `raw()` | `<C>Raw<'_>`: os membros como handles de porta (`raw().on` é um `CycleOutput<bool>`) | idem, com `history` |
| `read()` | `Result<<C>State>`: todos os membros; os de capacidades e os `@optional` como `Option<T>`; `Err(J-E0501)` com a instância `Failed` | idem |
| `health()` | `Health` | idem |
| `name()`, `index()` | nome da instância, posição no `Many` | idem |
| `has(cap)` | `bool`: a instância tem a capacidade | idem |

- Se um método tem o nome de um membro (em `led@1`, o `out on` e o `method on()`), o handle dá o método, e o membro fica em `raw()`.
- Os argumentos com unidade são `f64` na unidade declarada na classe (`run_at(pct: f64 %)` → `run_at(60.0)`).
- `MethodOutcome`: `outcomes() -> &[(&'static str, WriteOutcome<Value>)]`, `all_applied() -> bool`, `require_applied(self) -> Result<Self>`.
- `Health`: `Ok`, `Degraded(MemberSet)`, `Failed(MemberSet)` (17 §4.5). `MemberSet`: `iter() -> impl Iterator<Item = &'static str>`, `len()`, `is_empty()`, `Debug` com os nomes.
- `ClassHandle` é o trait comum: `name`, `index`, `health`, `read`, `has`. É sobre ele (ou sobre `<C>Cycle<'_>`) que se escrevem extension traits (14 §6.7).

Exemplo do que o `jkc` gera para `led@1` (excerto):

```rust
pub struct Led;                                      // declaração: #[device] led: Led
pub struct LedCycle<'a> { /* … */ }
impl<'a> LedCycle<'a> {
    pub fn on(&self);
    pub fn off(&self);
    pub fn set(&self, v: bool);
    pub fn toggle(&self);
    pub fn dim(&self, pct: f64);                     // capacidade dimmable; sem ela, J-E0502 no registo de decisões
    pub fn brightness(&self) -> Result<f64>;
    pub fn raw(&self) -> LedRaw<'a>;                 // raw().on: CycleOutput<'a, bool>
}
```

#### 4.8.3 `Many<C, MIN, MAX>`

A forma é `Many<Pump, 1, 8>`: dois parâmetros constantes. A escrita `Many<Pump, 1..=8>` dos documentos 04 e 14 não é Rust válido (um argumento genérico constante que não é literal nem bloco tem de ir entre chavetas), e o parser falha antes de a macro a ver (§10, S1).

```rust
impl<'a, H: ClassHandle, const MAX: usize> ManyHandle<'a, H, MAX> {
    pub fn len(&self) -> usize;                                  // fixo no init (14 §6.2, A10)
    pub fn is_empty(&self) -> bool;
    pub fn iter(&self) -> core::slice::Iter<'_, H>;              // ordem estável: a do bind
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, H>;
    pub fn get(&self, name: &str) -> Option<&H>;
    pub fn by_name(&self, name: &str) -> Result<&H>;             // Err(J-E0503)
    pub fn index(&self, i: usize) -> Option<&H>;
    pub fn names(&self) -> impl Iterator<Item = &str>;
    pub fn healthy(&self) -> impl Iterator<Item = &H>;           // health() == Ok
    pub fn read_all(&self) -> Result<BoundedVec<H::State, MAX>>; // Err se alguma obrigatória Failed
}

pub struct PerInstance<S, const MAX: usize> { /* … */ }          // estado por instância de um Many
impl<S: Default, const MAX: usize> PerInstance<S, MAX> {
    pub fn get(&self, h: &impl ClassHandle) -> &S;
    pub fn get_mut(&mut self, h: &impl ClassHandle) -> &mut S;
}
```

`PerInstance` é o que cumpre a A10 do documento 14: é guardado por **nome de instância** no `export-state`, e numa geração que acrescenta `p4` as instâncias `p1`–`p3` mantêm o seu estado e `p4` começa em `Default`.

#### 4.8.4 Implementações ativas (`#[joker::implements]`)

| Na vista | Tipo | Conteúdo |
|---|---|---|
| `io.cmd` | `<C>Cmd<'_>` | um handle de entrada por membro `out` da classe (nas capacidades oferecidas): o valor **aplicado** do comando virtual, depois do árbitro e do envelope (14 §7.2). Sem escritor, é o `safe` |
| `io.state` | `<C>StateOut<'_>` | um handle `set(v)` por membro `in` e `param` da classe: sinais publicados por esta app (registo `PUBLISH`) |
| `io.events` | `<C>Events<'_>` | um `emit(e)` por evento da classe |
| ações | `#[joker::on_action]` | §4.3.3 |

Um membro de uma capacidade que a implementação não oferece não existe em `io.state` nem em `io.cmd`: usá-lo é um erro de compilação.

#### 4.8.5 Classes locais (`#[derive(joker::Class)]`)

Uma struct com campos `#[input]` e `#[output]` torna-se uma classe local (`class_ref = "local:<Nome>"` em `joker:meta`). O derive gera `<Nome>Cycle<'a>` e `<Nome>Task<'a>` com os **mesmos nomes de campos**, cada um com o handle de porta da vista (`z.temp.get()?`, `z.setpoint.set(v)`), e `impl ClassHandle`. O comportamento livre escreve-se em `impl <Nome>Cycle<'_> { … }` no próprio crate (14 §6.7). Liga-se com `{ each, map }` (ref/01 §5.12.1).

#### 4.8.6 Membros de classe (handles aninhados)

Para cada membro de classe (documento 17, §8), o `jkc` gera um **campo público** no handle, com o handle da classe do membro. Não é um método: `io.esp.boot` é um `ButtonCycle<'_>`, e o resto é a API de sempre.

| Membro na classe | Campo em `<C>Cycle` / `<C>Task` | Exemplo |
|---|---|---|
| fixo (`member boot: std/button@1`) | `pub boot: ButtonCycle<'a>` | `io.esp.boot.clicked()?` |
| coleção (`member pumps: std/pump@1[1..8]`) | `pub pumps: ManyHandle<'a, PumpCycle<'a>, 8>` (§4.8.3) | `io.station.pumps.iter()`, `io.station.pumps.by_name("p1")?` |
| `@optional`, ou numa capacidade | `pub grid: Option<SgReadyCycle<'a>>` | `if let Some(g) = &io.hp.grid { … }` |

```rust
// o que o jkc gera para local/bench_io@1 (excerto)
pub struct BenchIoCycle<'a> {
    pub status: LedCycle<'a>,
    pub boot: ButtonCycle<'a>,
    pub knob: AnalogInCycle<'a>,
    /* … */
}
impl<'a> BenchIoCycle<'a> {
    pub fn rssi(&self) -> Result<i64>;
    pub fn uptime(&self) -> Result<f64>;
    pub fn online(&self) -> Result<bool>;
    pub fn raw(&self) -> BenchIoRaw<'a>;             // só os membros escalares
}
```

- Os métodos de um handle aninhado são os da classe do membro, e as escritas são do escritor da app, no seu nível, como se a app tivesse ligado a instância do membro diretamente (documento 16, P6): `io.hp.compressor.stop()` numa `heat_pump` composta é o mesmo registo `METHOD` que `io.comp1.stop()`.
- **Numa instância virtual**, os membros de classe só se leem, porque são da implementação (documento 16, P6). O handle aninhado tem o tipo de sempre (a classe não sabe se a instância é virtual), mas as portas de escrita desses membros ficam só de leitura na tabela de portas: um método chamado sobre eles, ou uma escrita por `raw()`, é descartado pelo host com `J-E0507` no registo de decisões, e não escreve. Uma classe cujos próprios métodos chamam métodos dos membros não pode ter instâncias virtuais (`J0656`, no `jk check`).
- `health()` do handle composto é a da árvore (17 §4.5); `Degraded(m)` e `Failed(m)` levam os nomes dos membros de classe.
- `read()` dá `<C>State` só com os membros escalares; cada membro de classe tem o seu `read()`.
- **Nomes:** um membro de classe partilha o espaço de nomes da classe (`JC0206`, `JC0207`), e por isso um campo nunca colide com um método gerado.
- **`joker:meta`** descreve a árvore: um membro de classe é um `member` com `member-kind` 6, a referência da classe, os limites da coleção e a sua própria lista de membros e métodos (§5, `spec/cddl/joker-meta.cddl`). A tabela de portas (§2.6.2, passo 2) desce a árvore **em profundidade**, pela ordem dos membros; numa coleção, por elemento. Cada nó da árvore tem uma entrada em `init-context.instances`, com `name` igual ao nome da instância ligada, ou ao caminho aninhado para um submembro sem nome (ref/03 §2.2.14).

### 4.9 Ações

```rust
impl<'a, A: Record, R: Record> ActionPort<'a, A, R> {
    pub fn invoke(&self, args: A) -> ActionCall<'a, R>;
}
pub struct ActionCall<'a, R> { /* … */ }
impl<'a, R: Record> ActionCall<'a, R> {
    pub fn key(self, key: &str) -> Self;                        // chave de idempotência; omissão: gerada
    pub async fn start(self) -> Result<ActionHandle<'a, R>, ActionError>;   // só em ações `long`
}
impl<'a, R: Record> core::future::IntoFuture for ActionCall<'a, R> { type Output = Result<R, ActionError>; }

impl<'a, R: Record> ActionHandle<'a, R> {
    pub fn progress(&self) -> Progress<'a>;                     // Stream<Item = (Option<f32>, BoundedString<256>)>
    pub fn cancel(&self);
    pub async fn finish(self) -> Result<R, ActionError>;
}
```

- `()` implementa `Record`, e por isso `Action<(), ()>` e `pump.reset().await` funcionam.
- **A chave de idempotência** (M19, E9) gerada por omissão é um ULID feito com `now()` e o gerador determinista: protege contra retransmissões do host, **não** contra a app que chama duas vezes. Quem quer idempotência de negócio passa uma chave sua com `.key(…)`. A mesma chave com argumentos diferentes dentro de `dedup_window` é `J-E0310`.
- Uma ação espera por resposta; uma tarefa de ciclo que precisa de uma publica um evento (02 §8).

### 4.10 Codificação dos registos

Os registos da app (`Internal<T>`, mensagens de `Mailbox`, eventos, argumentos e resultados de ações e métodos) viajam em **CBOR determinístico** (ref/00 §4.1, §7; D2), como um **mapa com chaves inteiras**: a chave de cada campo é a sua posição no esquema, **a contar de 0** (`joker:meta` para registos da app; o contrato `.jkc` para os de classes). `Plan { target_soc, limit_kw, valid_until }` codifica-se `{0: 0.62, 1: 400.0, 2: 1790244000}`. É a mesma forma no WIT, no Joker Link (ref/07 §6.2), no JDP e na ABI C.

- As interfaces de texto (API, `jk`) mostram os campos pelo nome, a partir do esquema.
- Um campo novo acrescenta-se **no fim**; reordenar ou remover um campo muda a versão do esquema (§4.12). Um campo `Option<T>` a `None` omite-se.
- `f64` e `f32` codificam-se com o menor formato de vírgula flutuante que preserva o valor (RFC 8949, §4.2.2); `Instant` é `[mono, utc, clock]`; `Quality` é `[code, reason]`; enums, o valor numérico.
- `Internal<T>`: o derive calcula o tamanho máximo codificado e o SDK recusa em compilação um registo acima de 64 B (`J-E0708`). `Plan` do documento 04 (3 campos) ocupa no máximo 31 B.

### 4.11 Log

`joker::log::{trace, debug, info, warn, error}!` (reexportados no prelúdio como `log::…`):

```rust
log::warn!("forecast OOD; a usar o plano conservador");
log::info!("nível {:.2} m", level; app_mode = "auto", pump = "p1");
```

Formatam num buffer de 1 KiB na pilha (sem alocação; truncado) e chamam `log.log`. Os campos depois de `;` são pares `nome = valor` (`Display`). Numa app que também dependa do crate `log`, usa-se `joker::log::warn!` explicitamente. Um `panic!` escreve a mensagem no log (`error`) antes do trap. `joker::log::enabled(Level) -> bool` é o `log.enabled` do WIT: as macros usam-no para não formatar mensagens que o host deitaria fora.

### 4.12 Estado, troca a quente e migração

- O estado de uma tarefa de ciclo (`st`) vive na memória da instância de ciclo e começa em `Default` ou no `init` da tarefa. Sobrevive entre ticks e a traps de outras tarefas da app, mas não a um trap que recrie a instância (crashloop, 01 §9).
- Sem `#[joker::state(persist)]`, uma versão nova começa com o estado inicial. Com ele, a troca a quente faz `export-state` na versão antiga e `migrate` na nova, com o blob `joker-state` do CDDL (§5): versão, versão da app, versão do esquema, tipo de instância e um CBOR por tarefa.
- A migração por omissão faz corresponder os campos por nome; os que faltam ficam em `Default` e os que sobram ignoram-se. Uma migração à medida:

```rust
#[joker::migrate(Ctl)]
fn migrate_ctl(prev: StateBlob<'_>, from_version: &str) -> Result<Ctl>
```

`StateBlob`: `version() -> u32`, `field<T: Record>(&self, name: &str) -> Option<T>`. Blob acima de 1 MiB: `J-E0915`.

### 4.13 Sem alocação no ciclo

A regra é a do invariante 6 do `AGENTS.md`, levada à instância Wasm:

| Momento | Alocação |
|---|---|
| `init`, `migrate`, `on-params` (ciclo) | permitida (estado inicial, tabelas do SDK) |
| `on-cycle` | **proibida**: o alocador global da instância de ciclo faz trap (`J-E0903`) |
| ABI canónico de `on-cycle` | a imagem e o resultado usam buffers reservados no `init` (`cabi_realloc` devolve-os); os valores de arena de `cycle-io.read` usam uma arena de 4 KiB reposta a cada tick |
| instância async | permitida (limite: `memory.max`, `J-E0906`) |

- Tudo o que a API de ciclo devolve é `Copy` ou limitado (`BoundedVec`, `BoundedString`); a formatação do log usa a pilha.
- Isto substitui o alocador bump reposto a cada tick do documento 04, §3.2: com ele, um `Vec` guardado no `st` ficava a apontar para memória reutilizada no tick seguinte, sem erro nenhum (§10, S3).
- O `joker-sdk` é `#![no_std]`. O `alloc` entra só nos backends `wasm` e `native-linux`, e só nas APIs async que devolvem coleções de tamanho variável (`History`, `Secret`, `Vec` do `kv`).

### 4.14 Joker Micro (backend nativo)

A mesma app compila para o MCU (19 §5), com a feature `backend-native` e o contrato `NativeApp` da ref/08 §2.11.5. O backend `native-micro` não copia a imagem: o núcleo dá ao SDK uma vista do snapshot com a mesma disposição de §3.2, e o SDK devolve o mesmo buffer de §3.3, que o `joker-core` drena como no Linux. Um *panic* de uma app num MCU é `jk_fatal` (D22): não há *sandbox*.

| API | Linux (Wasm) | Linux nativo | Joker Micro |
|---|---|---|---|
| tudo o que é de ciclo | sim | sim | sim |
| `set`, `write`, `relinquish`, `snapshot`, `subscribe`, eventos, mailbox, `emit`, métodos, `invoke`, `sleep` | sim | sim | sim |
| `every`, `on`, `on_event`, `startup` | sim | sim | sim |
| `at = "cron: …"` | sim | sim | sim, com `TIME` do Joker Link (sem ele, espera) |
| `kv` | 1 MiB | 1 MiB | 16 KiB |
| `secrets` | sim | sim | só em nós autónomos com segredos no blob (ref/02 §2.4.8); senão `J-E0408` |
| `history` | sim | sim | **`J0720`** |
| rede (`wasi:sockets`, `wasi:http`) | com `net.allow` | com `net.allow` | **`J0720`** |
| `Secret`, `History` e outras coleções com `alloc` | sim | sim | **`J0720`** |
| `#[joker::state(persist)]` | sim | não há troca a quente (04 §5): ignorado, com aviso | **`J0720`** |
| `ml`, `streams`, `blocks`, `scheduled-writes`, `intents` | 1.1 | 1.1 | **`J0720`** |

Os `J0720` são erros de compilação com o mesmo mecanismo de §4.4: os métodos existem, com um limite `where platform::Current: platform::Historian` que o alvo `joker_micro` não cumpre:

```
error[E0277]: J0720: `history()` is not available on Joker Micro
  --> apps/level/src/lib.rs:31:18
   |
31 |     let h = io.level.history(now - 24.h(), now, 15.mins()).await?;
   |                      ^^^^^^^ the MCU has no historian
   = help: use a core window in the manifest (`avg(level, 15m)`) or move this task to a Linux node
```

O **`J0721`** (aviso: `f64` num MCU sem FPU de dupla precisão, isto é, com `fpu = "none"` ou `"sp"`) é emitido pelo `jk build --target mcu:…` a partir das portas e dos campos `f64` em `joker:meta`, da FPU declarada pela placa (ref/03, §2.3.1; o enum da ref/08 §2.14) e das rotinas de vírgula flutuante por software que encontrar no objeto da app (ref/08 §2.11.5), e não pelo compilador. O `jk bench --target mcu` mede o custo real.

### 4.15 Funcionalidades instáveis

| Feature Cargo | Funcionalidade WIT | Dá |
|---|---|---|
| `unstable-ml` | `ml` | `#[model]`, `ModelHandle::infer`, `Tensor`, `Inference`, `InferError` |
| `unstable-streams` | `streams` | `StreamInput<F>`, `Frame` |
| `unstable-blocks` | `blocks` | `BlockInput`, `Block` |
| `unstable-scheduled-writes` | `scheduled-writes` | `write_at` |
| `unstable-intents` | `intents` | `propose`, `IntentId` |

Usar uma API destas sem a feature é `J-E0718` (com a feature a ativar). Com a feature, a funcionalidade entra em `joker:meta` (chave 10) e o `jokerd` de produção recusa o componente (`J-E0003`) até à versão que a estabiliza. Assinaturas previstas (podem mudar até à estabilização):

```rust
#[cfg(feature = "unstable-ml")]
impl<'a> ModelHandle<'a> {
    pub async fn infer<const K: usize>(&self, inputs: [Tensor; K]) -> Result<Inference, InferError>;
}
#[cfg(feature = "unstable-ml")]
impl Tensor {
    pub fn new(dtype: DType, shape: &[u32], data: &[u8]) -> Result<Self, InferError>;
    pub fn from_features<T: PortValue>(series: &History<T>, extra: &[f32]) -> Result<Self, InferError>;
    pub fn as_slice<E: Element>(&self) -> &[E];
    pub fn shape(&self) -> &[u32];
}
#[cfg(feature = "unstable-ml")]
pub struct Inference { pub outputs: Vec<Tensor>, pub quality: Quality, pub backend: String, pub latency: Duration }
```

### 4.16 Erros de compilação do SDK

As macros e os limites de trait emitem mensagens com código, localização no campo ou atributo culpado e uma ajuda. O `jk build` mostra-as no estilo do rustc e o `jk explain` explica-as.

| Código | Quando | Mensagem (primeira linha) |
|---|---|---|
| `J-E0701` | API async usada em `CycleIo` (ou de ciclo em `TaskIo`) | `` `{api}` is only available in async tasks (TaskIo) `` |
| `J-E0702` | `set` num `Internal<T>` na vista que não é a do escritor | `` `{port}` is written by the {domain} domain; declare #[internal(writer = "…")] to change it `` |
| `J-E0703` | porta numérica sem `unit` | `` numeric port `{port}` needs a unit; use unit = "1" for a dimensionless value `` |
| `J0109` | unidade fora do catálogo (16 §5) | `` unit `{unit}` is not in the v1 catalogue `` |
| `J-E0705` | literal de duração inválido | `` invalid duration `{lit}`; expected e.g. "200ms", "1.5s", "90m" `` |
| `J-E0706` | gama inválida, ou `default` fora dela | `` default {v} is outside range {range} `` |
| `J-E0707` | tipo de porta não suportado | `` `{ty}` is not a port type; use {suggestion} `` |
| `J-E0708` | `Internal<T>` acima de 64 B, ou com campos não escalares | `` internal record `{ty}` may encode to {n} bytes; the limit is 64 `` |
| `J-E0709` | nome de campo duplicado ou reservado | `` port name `{name}` is reserved `` |
| `J-E0710` | `name` ou `version` inválidos | `` app name `{name}` must match [a-z][a-z0-9_]{0,31} and not start with "joker" `` |
| `J-E0711` | duas `#[joker::app]`, ou fora da raiz do crate | `` a crate holds exactly one #[joker::app], at the crate root `` |
| `J-E0712` | assinatura de tarefa inválida | `` expected `fn(&mut CycleIo<App>, &mut State) -> Result<()>` `` (ou a de async) |
| `J-E0713` | gatilho inválido: zero ou dois, `on` para uma porta que não existe, `on_event` para uma porta que não é de eventos | `` a task needs exactly one trigger: every, on, on_event, at or startup `` |
| `J-E0714` | `period` em falta, `wcet > period`, período < 100 µs | `` cycle task `{task}` needs period = "…" `` |
| `J-E0715` | `#[implements]` com classe ou capacidade desconhecida | `` class `{class}` has no capability `{cap}` `` |
| `J-E0716` | ação de classe sem `#[joker::on_action]` (no `jk build`, a partir de `joker:meta`) | `` action `{action}` of `{class}` has no handler `` |
| `J-E0717` | limites de `Many` inválidos | `` Many<_, {min}, {max}>: expected 0 <= MIN <= MAX <= 1024 `` |
| `J-E0718` | API instável sem a feature | `` `{api}` is reserved for v1.1; enable the `unstable-{feature}` feature `` |
| `J-E0719` | tipo de estado de ciclo que não é `'static` | `` cycle state `{ty}` must be 'static `` |
| `J-E0720` | estado `persist` com um campo que não se codifica | `` field `{field}` of `{ty}` cannot be persisted `` |
| `J-E0721` | campo de registo de tipo não suportado | `` field `{field}`: `{ty}` is not a record field type `` |
| `J0720` | API indisponível no Joker Micro (§4.14) | `` `{api}` is not available on Joker Micro `` |

---

## 5. A secção `joker:meta`

O esquema completo está em `spec/cddl/joker-meta.cddl`. Aqui fica o que o esquema não diz.

### 5.1 Onde vive

| Artefacto | Local |
|---|---|
| componente de `jk build` | secção custom `joker:meta` ao nível do componente, com **um** item `joker-meta` (forma canónica) |
| componente de `cargo build` | a mesma secção no módulo core embutido, com a sequência de fragmentos (§5.2) |
| binário nativo (Micro, `kind = "native"`) | secção ELF `.joker_meta`, com a sequência de fragmentos |

Os leitores (`jokerd`, `jk inspect`, `jk check`, o agente) procuram primeiro ao nível do componente e depois no módulo core. Sem secção, ou com uma que não valida contra o CDDL, o componente é recusado (`J-E0004`). A chave 3 (mundo) tem de coincidir com o mundo que o componente realmente importa (`J-E0005`).

### 5.2 Fragmentos

Cada macro emite o seu fragmento (`meta-fragment`), e o ligador junta as secções com o mesmo nome. A secção é portanto uma **sequência CBOR** (RFC 8742) de fragmentos: um `app-fragment` (de `#[joker::app]`), um `task-fragment` por tarefa e um `schema-fragment` por tipo com derive.

### 5.3 Junção

1. Exatamente um `app-fragment` (`J-E0004`).
2. Tarefas: ordenadas por nome (ordem bytewise); o índice na lista é o `task` de `on-cycle` e `run-task`. Dois fragmentos com o mesmo nome: `J-E0004`.
3. Esquemas: ordenados por nome e deduplicados. O mesmo nome com conteúdo diferente: `J-E0004`. O nome é o identificador do tipo Rust; os esquemas gerados de classes têm o prefixo da classe (`"std/pump@1:tripped"`).
4. O resultado é codificado em CBOR determinístico. O `jk build` reescreve a secção com ele; o digest do componente cobre-o.

### 5.4 O que cada consumidor usa

| Consumidor | Chaves |
|---|---|
| `jk check` (ref/01) | 1, 2, 6 (ligações, unidades, gamas, `optional`), 7 (grupos, `J0130`), 8 (classes, `requires`, `Many`), 9 (implementações, `J0191`), 13 (segredos, `J0927`) |
| `jk build --target mcu` | 6, 11 (tipos `f64`, `J0721`), 10 (instáveis, `J0720`) |
| `joker.lock` (ref/02) | 2 (`version`), 3 (`wit`) |
| `jokerd` ao carregar | 3 (mundo, §2.8), 5 (ABI), 10 (`J-E0003`), 6–9 (tabela de portas, §2.6.2), 16 (memória) |
| agente e Deck (ref/10) | 2, 3, 10, antes do deploy |
| `jk inspect`, API (ref/09) | tudo |

### 5.5 Exemplo

A app `level` do exemplo 00, forma canónica (168 B), em notação de diagnóstico CBOR:

```
{
  0: 1,
  1: "level",
  2: "0.1.0",
  3: "joker:runtime@0.1.0",
  4: {0: "rust", 1: "0.1.0", 2: 0},
  5: 1,
  6: [
    {0: "level", 1: 0, 2: 3, 3: "m"},
    {0: "pump", 1: 1, 2: 3, 3: "%"},
    {0: "target", 1: 2, 2: 3, 3: "m", 7: {0: 0, 1: 3}, 8: 1.5},
    {0: "kp", 1: 2, 2: 3, 3: "1", 7: {0: 0, 1: 500}, 8: 80.0}
  ],
  7: [{0: "control", 1: 0, 2: 100000000, 3: 10000}],
  8: [],
  10: [],
  11: []
}
```

---

## 6. `joker-test`

### 6.1 Modos

| Comando | O que corre | Quando |
|---|---|---|
| `jk test` | o componente `.wasm` real no wasmtime (o `jk` compila-o e passa-o ao binário de testes em `JOKER_TEST_COMPONENT`), com o `joker-core` real e relógio virtual | sempre em CI; é o que conta (04 §3.7) |
| `cargo test` | o mesmo código da app com o backend `native-linux`, ligado ao binário de testes | depuração rápida; o harness escreve um aviso de que não é o componente de produção |

Nos dois modos: **relógio virtual**, núcleo real (árbitro, envelope, qualidade), determinismo total. Depois de cada tick, o harness corre o executor async até todas as tarefas estarem à espera de um temporizador ou de um gatilho; as propostas async entram no passo 2 do tick seguinte.

### 6.2 Construção

```rust
pub struct Harness<A: App> { /* … */ }
impl<A: App> Harness<A> {
    pub fn new() -> Self;
    pub fn group(self, task: &str, period: Duration) -> Self;            // período do grupo desta tarefa
    pub fn param(self, name: &str, v: impl Into<ParamValue>) -> Self;     // 400.0, true, "400 kW" (ref/03 §2.1.3)
    pub fn input(self, port: &str, s: impl Into<Series>) -> Self;         // 0.5 é Series::constant(0.5)
    pub fn internal<T: Record>(self, port: &str, v: T) -> Self;
    pub fn command(self, member: &str, v: impl Into<Value>) -> Self;      // comando virtual de uma implementação
    pub fn device(self, name: &str, d: SimDevice) -> Self;                // instância simulada para um #[device]
    pub fn devices(self, name: &str, ds: impl IntoIterator<Item = SimDevice>) -> Self; // Many
    pub fn safe(self, output: &str, v: impl Into<Value>) -> Self;         // omissão: T::default()
    pub fn envelope(self, output: &str, e: Envelope) -> Self;             // omissão: só o safe
    pub fn lease(self, d: Duration) -> Self;                              // omissão: 3 × período (16 A9)
    pub fn level(self, level: &str) -> Self;                              // omissão: "control:12"
    pub fn manual(self, output: &str, v: impl Into<Value>, at: Duration, ttl: Duration) -> Self; // override
    pub fn on_action(self, port: &str, r: ActionResponder) -> Self;       // resposta de uma ação chamada
    pub fn history(self, port: &str, s: impl Into<Series>) -> Self;       // dados para history()
    pub fn kv(self, key: &str, v: &[u8]) -> Self;
    pub fn secret(self, name: &str, v: &[u8]) -> Self;
    pub fn start_utc(self, rfc3339: &str) -> Self;                        // omissão: "2026-01-01T00:00:00Z"
    pub fn clock(self, q: ClockQuality) -> Self;
    pub fn seed(self, seed: u64) -> Self;
    pub fn manifest(self, path: &str) -> Self;                            // envelopes e ligações de um manifesto real
    pub fn scenario(self, path: &str) -> Self;                            // cenário de ref/03
}
```

`Envelope`: `Envelope::new().range(lo, hi).rate(per_s).safe(v).on_interlock_hold()`. `ActionResponder`: `ActionResponder::ok(value)`, `::err(code)`, `::after(d, value)`, `::with(|args| …)`.

### 6.3 Execução

```rust
impl<A: App> Harness<A> {
    pub fn run_for(&mut self, d: Duration);
    pub fn run_until(&mut self, t: Duration);        // tempo virtual desde o início
    pub fn step(&mut self);                           // um tick do grupo mais rápido
    pub fn now(&self) -> Instant;
    pub fn set_input(&mut self, port: &str, v: impl Into<Value>);
    pub fn inject(&mut self, port: &str, q: Quality, for_: Duration);
    pub fn set_param(&mut self, name: &str, v: impl Into<ParamValue>);   // chama on-params
    pub fn emit(&mut self, port: &str, e: impl Record);                  // evento de entrada
    pub fn stop_app(&mut self);                                          // as duas instâncias param
    pub fn restart_app(&mut self);
    pub fn hot_swap(&mut self) -> Result<()>;                            // export-state + migrate
}
```

### 6.4 Resultados e asserções

```rust
impl<A: App> Harness<A> {
    pub fn output(&self, port: &str) -> OutputTrace;
    pub fn input_series(&self, port: &str) -> SeriesTrace;
    pub fn device(&self, name: &str) -> DeviceTrace;     // .member("on") → OutputTrace
    pub fn events(&self, port: &str) -> EventTrace;
    pub fn alarm(&self, name: &str) -> AlarmTrace;
    pub fn published(&self, port: &str) -> SeriesTrace;
    pub fn internal(&self, port: &str) -> SeriesTrace;
    pub fn actions(&self, port: &str) -> ActionTrace;
    pub fn traps(&self) -> &[Trap];                      // Trap { at, task, code, message }
    pub fn errors(&self) -> &[TaskError];                // Err devolvidos
    pub fn logs(&self) -> &[LogLine];
}

impl OutputTrace {
    // janelas (devolvem outra OutputTrace)
    pub fn after(&self, t: Duration) -> OutputTrace;
    pub fn before(&self, t: Duration) -> OutputTrace;
    pub fn between(&self, a: Duration, b: Duration) -> OutputTrace;
    pub fn when(&self, c: Condition) -> OutputTrace;
    // sobre o valor aplicado
    pub fn values(&self) -> impl Iterator<Item = Value> + '_;
    pub fn all(&self, f: impl FnMut(Value) -> bool) -> bool;
    pub fn any(&self, f: impl FnMut(Value) -> bool) -> bool;
    pub fn at(&self, t: Duration) -> Observed;
    pub fn last(&self) -> Observed;
    pub fn toggles(&self) -> usize;
    pub fn intervals(&self) -> impl Iterator<Item = Duration> + '_;
    pub fn toggles_every(&self, d: Duration) -> bool;    // todos os intervalos iguais a d
    // sobre as propostas e os resultados
    pub fn proposals(&self) -> impl Iterator<Item = (Duration, Value)> + '_;
    pub fn no_new_writes(&self) -> bool;
    pub fn outcomes(&self) -> impl Iterator<Item = (Duration, WriteOutcome<Value>)> + '_;
}

impl SeriesTrace {
    pub fn lt(&self, v: impl Into<Value>) -> Condition;
    pub fn le(&self, v: impl Into<Value>) -> Condition;
    pub fn gt(&self, v: impl Into<Value>) -> Condition;
    pub fn ge(&self, v: impl Into<Value>) -> Condition;
    pub fn eq(&self, v: impl Into<Value>) -> Condition;
    pub fn bad(&self) -> Condition;
    pub fn values(&self) -> impl Iterator<Item = Value> + '_;
}
// Condition: and, or, not.

pub struct Observed(/* Option<Value> */);
impl<T: Into<Value> + Copy> PartialEq<Option<T>> for Observed { /* … */ }  // assert_eq!(h.output("led").last(), Some(false))
```

Os traços não são genéricos: um `Value` compara-se com `f64`, `bool`, `i64` e `u64` (`all(|v| v == 0.0)`), e `Observed` compara-se com `Option<T>`. Assim `h.output("led")` não precisa de anotação de tipo. Uma porta desconhecida faz `panic!` com a lista das portas da app.

### 6.5 Séries

```rust
impl Series {
    pub fn constant(v: impl Into<Value>) -> Self;
    pub fn step(before: impl Into<Value>, after: impl Into<Value>, at: Duration) -> Self;
    pub fn ramp(from: f64, to: f64, over: Duration) -> Self;       // linear e depois fica em `to`
    pub fn linear(at_0: f64, at_over: f64, over: Duration) -> Self; // reta que continua depois de `over`
    pub fn sine(mean: f64, amp: f64, period: Duration) -> Self;
    pub fn noise(mean: f64, sd: f64) -> Self;                      // gerador determinista (seed)
    pub fn points(p: &[(Duration, f64)]) -> Self;                  // degraus nos instantes dados
    pub fn replay(path: &str) -> Self;                             // .parquet ou .jktrace
    pub fn every(self, d: Duration) -> Self;                       // amostra nova a cada d (omissão: cada tick)
    pub fn with_quality_at(self, t: Duration, q: Quality) -> Self; // a partir de t
    pub fn with_quality(self, from: Duration, to: Duration, q: Quality) -> Self;
    pub fn frozen_after(self, t: Duration) -> Self;                // valor bit a bit igual
}
```

O prelúdio de `joker-test` reexporta `QualityReason::*`, e por isso `Quality::bad(CommFailure)` funciona sem prefixo.

### 6.6 Dispositivos simulados

`SimDevice` é uma instância de classe simulada para o harness, com o simulador da classe (14 A13; ref/03 §2.4.11) quando existe:

```rust
impl Sim {
    pub fn of<C: ClassDecl>() -> SimDevice;              // qualquer classe, membros a Default
    pub fn temp_sensor() -> SimDevice;                   // um por classe std com simulador (gerado)
    pub fn button() -> SimDevice;
    pub fn pump() -> SimDevice;
}
impl SimDevice {
    pub fn member(self, name: &str, s: impl Into<Series>) -> Self;
    pub fn constant(self, v: impl Into<Value>) -> Self;  // o membro principal da classe
    pub fn press_at(self, at: Duration, hold: Duration) -> Self;     // button
    pub fn fault(self, member: &str, q: Quality, at: Duration, for_: Duration) -> Self;
    pub fn capabilities(self, caps: &[&str]) -> Self;
    pub fn name(self, n: &str) -> Self;
}
```

### 6.7 Cenários e conformidade de classes

- `Harness::scenario(path)` carrega um cenário de `sim/scenarios/*.scenario.toml` (ref/03) contra a app sozinha: as ações `[[set]]`, `[[press]]` e `[[inject]]` são aplicadas, e as expectativas `[[expect]]` são verificadas por `h.check() -> ScenarioReport` (`passed() -> bool`, `failures() -> &[Failure]`).
- `joker_test::conformance::<C>(path, harness)` corre os casos `classes/<pacote>/<nome>.conformance.toml` contra uma app que implementa `C`; é o que o `jk class verify` usa para uma instância virtual.
- Os valores esperados escrevem-se à mão, e nunca a partir de uma execução anterior (a regra do documento 16, §8).

---

## 7. Erros de runtime `J-E…`

Gama deste documento: `J-E0001`–`J-E0799` e `J-E0900`–`J-E0999`. Todos entram no registo `codes.toml` (16 §9). Os traps (`J-E09xx`) contam para o crashloop (01 §9: 3 em 10 s); os outros são valores de `error.code`.

| Código | Gravidade | Mensagem | `ActionErrorKind` / nota |
|---|---|---|---|
| `J-E0001` | error | component world is newer than this runtime | recusado antes do deploy e ao carregar |
| `J-E0002` | error | component world or cycle ABI is outside the supported window | §2.8, §3.1 |
| `J-E0003` | error-in-prod | component uses an unstable feature | §2.9 |
| `J-E0004` | error | joker:meta missing or invalid | §5 |
| `J-E0005` | error | joker:meta world does not match the component imports | |
| `J-E0006` | error | component imports an interface that is not allowed | §2.7 |
| `J-E0007` | error | app name in joker:meta does not match the manifest | também `jk check` |
| `J-E0008` | error | init-context rejected by the SDK (layout mismatch) | §2.6.2 |
| `J-E0011` | error | app refused the new parameters | §4.3.4 |
| `J-E0050` | error | the app returned an error | `Error::app` |
| `J-E0101` | error | value unavailable: quality is not Good | `get()` |
| `J-E0102` | error | value unavailable: quality is Bad | `get_uncertain()` |
| `J-E0103` | error | port is not bound | porta `optional` |
| `J-E0104` | error | value type does not match the port | tier 2 |
| `J-E0105` | error | record does not decode against its schema | |
| `J-E0203` | error | proposal buffer full; later proposals dropped | §3.4 |
| `J-E0204` | error | write was not applied | `require_applied()` |
| `J-E0205` | error | proposal-only writer cannot write directly | M18; `J0212` no `jk check` |
| `J-E0206` | error | write cancelled: the generation changed before the commit | |
| `J-E0207` | error | scheduled-write queue full | 1.1 |
| `J-E0209` | error | this writer cannot create intents | 1.1 |
| `J-E0210` | error | intent TTL above max_manual | 1.1 |
| `J-E0301` | error | action denied by its permissive | `Denied` |
| `J-E0302` | error | action not allowed for this caller's role | `Forbidden` |
| `J-E0303` | error | action timed out | `Timeout` |
| `J-E0304` | error | device reported an error | `Device` |
| `J-E0305` | error | invalid action arguments | `Invalid` |
| `J-E0306` | error | action queue of the device is full | `Busy` |
| `J-E0307` | error | device not connected | `NotConnected` |
| `J-E0308` | error | action cancelled | `Cancelled` |
| `J-E0309` | error | invalid idempotency key | `Invalid` |
| `J-E0310` | error | idempotency key reused with different arguments | `Conflict` |
| `J-E0311` | error | the instance does not support this action | `Unsupported` |
| `J-E0312` | error | the action implementation trapped | `Failed` |
| `J-E0313` | error | not a long action | `Invalid` |
| `J-E0401` | error | invalid history range | |
| `J-E0402` | error | history result too large | |
| `J-E0403` | error | signal is not recorded | |
| `J-E0404` | error | kv quota exceeded | |
| `J-E0405` | error | invalid kv key | |
| `J-E0406` | error | kv value too large | |
| `J-E0407` | error | kv storage error | |
| `J-E0408` | error | secret not declared or not bound | |
| `J-E0409` | error | secret not found on this node | |
| `J-E0410` | error | event payload too large or invalid | |
| `J-E0411` | warning | event dropped for a full consumer | contado |
| `J-E0412` | warning | mailbox full or message too large | contado |
| `J-E0413` | warning | network destination not allowed | log do host |
| `J-E0414` | error | too many ports in one request | |
| `J-E0501` | error | instance health is Failed | `read()?` |
| `J-E0502` | error | the instance lacks this capability | |
| `J-E0503` | error | no instance with this name in the Many | `by_name` |
| `J-E0504` | error | invalid method arguments | registo de decisões |
| `J-E0505` | error | method not in the class contract | tier 2 |
| `J-E0506` | error | member not bound on this instance | |
| `J-E0507` | error | write to a member of a virtual instance: the implementation owns it | registo de decisões; 16 P6 |
| `J-E0601` | error | feature not available in this runtime | stubs de §2.9 |
| `J-E0602` | error | inference deadline exceeded | 1.1 |
| `J-E0603` | error | inference backend error | 1.1 |
| `J-E0604` | error | tensor shape or dtype mismatch | 1.1 |
| `J-E0605` | error | tensor too large for the Wasm boundary (1 MiB) | 1.1 |
| `J-E0606` | error | frame expired | 1.1 |
| `J-E0607` | warning | blocks lost (slow consumer) | 1.1 |
| `J-E0701`…`J-E0721` | error | erros de compilação | §4.16 |
| `J-E0901` | trap | async API called from the cycle instance | `async-io`, `kv`, `secrets`, `ml`, `frames`, `blocks` |
| `J-E0902` | trap | cycle API called from the async instance | `cycle-io` |
| `J-E0903` | trap | heap allocation during a cycle task | §4.13 |
| `J-E0904` | trap | cycle task deadline exceeded | epoch (01 §4) |
| `J-E0905` | trap | guest panicked or called exit | |
| `J-E0906` | trap | memory limit exceeded | `memory.max` |
| `J-E0907` | trap | malformed proposal buffer or invalid port use | §3.4 |
| `J-E0908` | error | app is in crashloop | alarme `joker.app.<app>.crashloop` |
| `J-E0909` | error | init failed or timed out | |
| `J-E0910` | trap | stack overflow | |
| `J-E0911` | trap | fuel exhausted | só testes com fuel |
| `J-E0914` | error | migrate failed; previous version kept | |
| `J-E0915` | error | exported state too large | 1 MiB |

### 7.1 Erros de ações na API

Uma ação invocada pela API (`POST /v1/actions/{target}/{action}`, ref/09 §5.10) passa pelo mesmo caminho que um `invoke` de uma app. O resultado de erro que a app recebe e o que a API devolve são o mesmo facto com dois códigos:

| `invoke` (app) | `ActionErrorKind` | API: forma | API: código | HTTP |
|---|---|---|---|---|
| `J-E0301` permissivo falso | `Denied` | erro do pedido | `J-E1402` | 409 |
| `J-E0302` papel | `Forbidden` | erro do pedido | `J-E1002` | 403 |
| `J-E0303` timeout | `Timeout` | `Invocation` `failed`, `error.kind = timeout` | `J-E1406` | 200 |
| `J-E0304` erro do equipamento | `Device` | `failed`, `kind = device` | `J-E1407` | 200 |
| `J-E0305` argumentos | `Invalid` | erro do pedido | `J-E1401` | 400 |
| `J-E0306` fila cheia | `Busy` | erro do pedido | `J-E1403` | 503 |
| `J-E0307` dispositivo desligado | `NotConnected` | `failed`, `kind = device` | `J-E1415` | 200 |
| `J-E0308` cancelada | `Cancelled` | `failed`, `kind = cancelled` | `J-E1409` | 200 |
| `J-E0309` chave inválida | `Invalid` | erro do pedido | `J-E1025` | 400 |
| `J-E0310` chave repetida com outros argumentos | `Conflict` | erro do pedido | `J-E1024` | 422 |
| `J-E0311` a instância não tem a ação | `Unsupported` | erro do pedido | `J-E1400` | 404 |
| `J-E0312` a implementação fez trap | `Failed` | `failed`, `kind = device` | `J-E1408` | 200 |
| `J-E0313` não é uma ação longa | `Invalid` | erro do pedido | `J-E1405` | 409 |

Um `Err` devolvido por um `on_action` chega à API como `failed` com o código que a app escolheu, e `kind` derivado dele por esta tabela (`device` para um código fora dela).

---

## 8. Comportamento esperado

1. **Uma chamada por tarefa e por tick.** O host chama `on-cycle` pela ordem do grupo (`order`, 01 §4 passo 4); o passo 4b (comandos virtuais) corre sobre os registos devolvidos logo a seguir à tarefa que os produziu.
2. **Escritor único por app** (16, A1): as propostas das duas instâncias contam como o mesmo escritor e o mesmo nível; entre elas, vale A4.
3. **Leitura:** a instância de ciclo lê sempre do latch (02 §6.1), e uma saída mostra o valor aplicado no tick anterior. A async lê o slot atual (02 §6.4).
4. **Qualidade:** a tabela do 16 §3.1 aplica-se tal como está a `get`, `get_uncertain`, `get_or` e `sample`, nas duas vistas e nos handles de classe. Nenhuma API do SDK converte uma amostra `Bad` num número sem `Result` (invariante 4).
5. **Escritas async:** `set` e `write` entram na inbox do grupo dono e são aplicadas no próximo tick dele (01 §5). O `WriteTicket` resolve depois desse commit com o resultado final; entretanto, `last_outcome()` dá `Deferred`.
6. **Métodos:** a expansão é do núcleo (17 §4.1). No tick da chamada, `pending()` dos `out` expandidos dá `None`; o resultado vê-se no tick seguinte com `last_outcome()`, ou no `await` em async.
7. **Ações:** nunca no ciclo; serializadas por dispositivo; a chave deduplica dentro de `dedup_window` (16 E9); o manifesto vence a classe na configuração (17 §4.2).
8. **Traps:** descartam as propostas desse tick dessa app; os leases continuam a correr (01 §9). Um `Err` não é trap.
9. **Parâmetros:** mudam num limite de tick, iguais para todas as tarefas desse tick.
10. **Determinismo:** o que a instância de ciclo vê (imagem, `now`, `random`, WASI virtualizado) é função do traço e da geração; com NaN canonicalizados e sem relaxed SIMD (09, ataque 10), o mesmo `.wasm` dá os mesmos bytes de propostas em aarch64 e x86_64.
11. **Limites** (ref/00 §10 e os desta referência): portas por app 65 535; buffer de propostas 16 KiB (máx. 1 MiB); `args` 4 KiB; evento 8 KiB; mensagem de mailbox 256 B; `snapshot`/`subscribe` 256 portas; `history` 10 000 pontos; kv como em §2.5.1.
12. **Compatibilidade:** §2.8 e §2.9. Uma app 1.y corre em todas as 1.x com x ≥ y, com as mesmas propostas para as mesmas entradas.

---

## 9. Cobertura dos exemplos

Os 39 blocos `rust` de `docs/**/*.md` fora de `docs/ref/`, extraídos por script. "✓" quer dizer que tudo o que o bloco usa está nesta referência (ou na ref/08, para as apps de MCU, e nas classes `std/` do documento 14); "erro" aponta a correção que o exemplo precisava. As linhas 1–23 são dos documentos e exemplos, tal como estavam quando esta referência foi escrita, e as correções foram aplicadas na integração da rodada da API (documento 22, §7). As linhas 24–39 são da trilha reescrita (onze partes, documento 23), verificadas por um segundo script que procura cada atributo, método e tipo do bloco nesta referência e na ref/08; os únicos nomes que não estão cá são membros de classes (`pressed`, `blink`, `alert`, `LightMode::*`), que vêm do código gerado da `.jkc`.

| # | Bloco | Usa | Estado |
|---|---|---|---|
| 1 | 00 §"DX" (l. 74) | `#[joker::app]` sem argumentos, `#[input/output/param(unit, default)]`, `#[joker::cycle(period)]`, `get()?`, `set` | ✓ |
| 2 | 04 §3.1 | `app(name, version)`, `unit`/`quantity`/`range`/`default`, `#[internal] Internal<Plan>`, `#[model]`, `#[alarm] Alarm`, `#[output(event)] EventOutput`, `derive(Record)`, `UtcSeconds` | ✓; `#[model]` é 1.1 (`unstable-ml`); **erro**: `LowSoc` não está definido (falta `#[derive(joker::Event)] pub struct LowSoc { soc: f64 }`) |
| 3 | 04 §3.2 | `get()?`, `get_or_default()`, `now().utc_seconds()`, `raise/clear`, `dt()`, `set` | ✓ |
| 4 | 04 §3.3 | `task(every, jitter)`, `history(…)`, `infer`, `Tensor::from_features`, `log::warn!`, `task(on)`, `Sample<f64>`, `emit` | **erro**: `15.min()` → `15.mins()`; **erro**: `io.calendar_features(now)` não existe (função da app: `calendar_features(now)`); `infer`/`Tensor` são 1.1 |
| 5 | 04 §3.5 | `#[input(event = "Detections")]`, `#[action] Action<(), ()>`, `task(on_event)`, `write_at` | **erro**: `event` é uma flag (`#[input(event)]`), o tipo vem do genérico; **erro**: `Class::Defect` não está definido; `write_at` é 1.1 |
| 6 | 04 §3.6 | `joker::classes::std`, `#[device]`, `requires`, `Many` | **erro**: `Many<Pump, 1..=8>` → `Many<Pump, 1, 8>` |
| 7 | 04 §3.7 | `Harness`, `group`, `param`, `input`, `Series::ramp/linear/constant`, `internal`, `run_for`, `output`, `input_series`, `when`, `lt`, `all`, `traps`, `with_quality_at`, `Quality::bad(CommFailure)`, `after`, `no_new_writes` | **erro**: `10.min()` → `10.mins()` (três vezes) |
| 8 | 06 §4 | trait `InferenceBackend` | fora (06, interno) |
| 9 | 06 §6 | `infer`, `y.quality.code` | 1.1; **erro**: `Good =>`, `Uncertain =>`, `Bad =>` são padrões de ligação que apanham tudo: `QualityCode::Good` etc. |
| 10 | 14 §6.1 | `#[device] Led, Button`, `clicked()?`, `toggle()` | ✓ |
| 11 | 14 §6.2 | `Many`, `requires`, `Tank`, `Meter` | **erro**: `Many<…, 1..=8>`; **erro**: `Meter` → `MeterAc3` (ou `use … MeterAc3 as Meter`); `tank@1` não está na `std/` (14 §10) |
| 12 | 14 §6.3 | `get("p1").read()?`, `PumpState`, `Health`, `Error::quality` | **erro**: `get` devolve `Option`: `io.pumps.by_name("p1")?.read()?` |
| 13 | 14 §6.5 | `derive(joker::Class)`, `Many<Zone, 1..=64>` | **erro**: `Many<Zone, 1, 64>` |
| 14 | 14 §6.7 | extension trait, `Instant`, `500.ms()`, `toggle()` | **erro**: `impl LedExt for Led<'_>` → `for LedCycle<'_>` |
| 15 | 14 §7.1 | `#[joker::implements(Pump, capabilities)]`, `cycle(init = …)`, `io.cmd`, `io.state` | **erro**: `io.state.speed` é da capacidade `variable_speed`, que a `pump_ctl` não oferece (só `hour_counter`): não compila |
| 16 | 14 §9.1 | `Many`, `healthy()`, `index()`, `iter_mut()`, `len()`, `InputMany`, membros como getters | **erro**: `Many<…, 1..=8>`/`0..=16`; **erro**: `io.pumps.len()` dentro do `for … in io.pumps.iter_mut()` é E0502 (use `iter()`, os métodos são `&self`, ou calcule `len` antes); `FlowMeter`, `Tank` não estão na `std/` |
| 17 | 14 §9.2 | `Many`, `iter().filter`, `served_by()`, `ahu.supply_sp()?`, `set_supply_sp().await?` | **erro**: `Many<…>`; `Ahu`/`Vav` → `HvacAhu`/`HvacVav`; **erro**: `v.served_by() == ahu.name()` compara `Result` com `&str` (`v.served_by().is_ok_and(|s| s == ahu.name())`) |
| 18 | 14 §13 | `#[joker::method(writes = …)]` | alternativa futura, não é API (§11) |
| 19 | 19 §5 | `Input<f32>`, `Param<f32>`, backend nativo | ✓ |
| 20 | exemplo 00 | `range`, `unit = "1"`, `wcet` | ✓ |
| 21 | exemplo 01 (curtail) | `#[output(unit, many)] OutputMany`, `iter_mut`, `set` | ✓ |
| 22 | exemplo 01 (Plan) | `derive(Record)`, `utc_seconds() + 30 * 60`, `Tensor::as_slice` | ✓; `Tensor` é 1.1 |
| 23 | exemplo 02 | `derive(Event)`, `MonoNanos`, `BoundedVec`, `Quality` em registos, `#[input(event)]`, `#[publish] Counter`, `inc()`, `write_at` | ✓; `write_at` é 1.1; **erro**: `Class` não está definido (`#[derive(joker::Enum)] pub enum Class { Ok = 0, Defect = 1 }`) |
| 24 | trilha 1 §5 | `app(name, version)`, `#[output]`, `#[param(unit, range, default)]`, `cycle(period, wcet)`, `dt()`, `set`, estado `Default` | ✓ |
| 25 | trilha 1 §9 (testes) | `group`, `param`, `run_for`, `output`, `toggles`, `toggles_every`, `safe`, `stop_app`, `ms()`/`s()` | ✓ |
| 26 | trilha 3 §3 | `#[input(unit)]` em `°C`, `#[param(unit, range, default)]` em `°C` e `ms`, `get()?` (`Err` sem temperatura `good`) | ✓ |
| 27 | trilha 3 §7 (testes) | `input`, `Series::constant`, `with_quality`, `Quality::bad`, `after`, `between`, `errors`, `toggles_every` | ✓ |
| 28 | trilha 4 §7 | `cycle(init = …)`, `Input<bool>`, `Default::default` | ✓ |
| 29 | trilha 5 §3 | `#[joker::state(persist)]`, `get_or`, `#[param(unit, default)]` | ✓ |
| 30 | trilha 5 §7 | `sample()`, `quality.is_good()` | ✓ |
| 31 | trilha 7 §6 | `joker::classes::std`, `#[device]` (`Led`, `Button`, `TempSensor`, `AnalogIn`), `clicked()`, `pressed()`, `temperature()`, `raw()`, `off()`, `set`, `get_or` | ✓; os membros são os das classes `std/` (documento 14) |
| 32 | trilha 8 §4 | `#[joker::implements(StatusLight)]`, `#[device] Led`, o `enum LightMode` gerado da `.jkc`, `Option` no estado | ✓; `LightMode` e os membros vêm do código gerado da classe `local/` (§4.8.4) |
| 33 | trilha 8 §4 (testes) | `group`, `device`, `command`, `member`, `after`, `toggles_every`, `Sim::of` | ✓ |
| 34 | trilha 8 §6 | `#[device] sinal: StatusLight`, métodos `blink`/`off`, `clicked()`, `pressed()`, `temperature()`, `raw()` | ✓ |
| 35 | trilha 10 §2 | app nativa de MCU: `app`, `#[input]`, `#[output]`, `#[param(unit, range, default)]`, `cycle(period)` sem `wcet`, `dt().as_secs_f32()` | ✓ (ref/08 §2.11; `as_secs_f32` é de `core::time::Duration`) |
| 36 | trilha 10 §4 | `#[joker::task(every)]`, `history`, `now()`, `h()`, `mins()` num alvo MCU | erro de propósito: `J0720` (`history` não existe no Joker Micro, ref/08 §2.11.5) |
| 37 | trilha 10 §4 (linha) | `#[param(unit, range, default)]` com `Param<f64>` num MCU com `fpu = "sp"` | aviso de propósito: `J0721` |
| 38 | trilha 11 §4 | `#[input] Input<bool>` ligado a `alarm.<a>.active`, `get_or`, método `alert()` da classe | ✓ |
| 39 | trilha 11 §5 | função Rust pura (`Vec<f64>`) | ✓; a 0.7.1 compila e passa os testes: o defeito é de tempo (overruns no Pi Zero W), e não de API |

Menções fora de blocos: `#[joker::state(persist)]` (§4.12), `Mailbox<T, N>` (§4.2), `#[publish] Published<T>`/`Counter` (§4.2), `InputMany`/`OutputMany` (§4.6), `WriteTicket` (§4.6), `ActionHandle` (§4.9), `raw()` (§4.8.2), `x.mine()`/`x.winner()` (§4.6). **Erro:** o `#[joker::shared]` do documento 04 §3.4 não existe e o erro prometido não é implementável (um `static` mutável não se distingue de outro); a separação das memórias é garantida pelas duas instâncias, e documenta-se. **Erro:** a saída do `jk inspect` da trilha antiga (parte 6, §5: "métodos usados", "consultas usadas") não é obtenível sem análise estática; `joker:meta` lista os métodos e membros **conhecidos** pelo handle.

---

## 10. Estresse

**S1 — `Many<Pump, 1..=8>`.** Os documentos 04 e 14 escrevem-no assim em sete sítios. Um argumento genérico constante que não é literal nem bloco tem de ir entre chavetas, e o erro sai do parser do rustc, antes de o `#[joker::app]` receber os tokens. *Quebrou.* **Correção:** `Many<C, MIN, MAX>` com dois `usize` (§4.8.3), que é Rust estável, dá a capacidade de `read_all()` em compilação e é o que o `jk check` compara (`J0610`).

**S2 — `15.min()`.** Com `DurationExt` para `u64` no prelúdio, `15.min()` tem dois candidatos (`Ord::min` e `DurationExt::min`), e é o erro E0034; em `f64`, o método inerente `f64::min` ganha e pede um argumento. *Quebrou.* **Correção:** `mins()` (§4.7). O `ms()`, o `s()`, o `h()` e o `d()` não colidem com nada.

**S3 — Um `Vec` no estado de ciclo.** Com o alocador bump reposto a cada tick (documento 04, §3.2), `st.buf.push(x)` num tick guardava um ponteiro para memória que o tick seguinte reutilizava: corrupção silenciosa e não determinista. *Quebrou.* **Correção:** no `on-cycle`, o alocador faz trap (`J-E0903`), e o harness mostra o tick e a pilha; o `init` pode alocar; a API de ciclo só usa tipos limitados (§4.13).

**S4 — `on-cycle` com `Err`.** A assinatura do documento 04 (`result<list<u8>, string>`) perdia o buffer quando a tarefa devolvia `Err`, contra a regra de manter as propostas já feitas. *Quebrou.* **Correção:** `cycle-result { proposals, error: option<error> }` (§2.6.1).

**S5 — `write(v).await?` não quer dizer "escrito".** `Rejected(Interlock)` é um resultado, e não um erro de transporte: `?` deixa-o passar. É o erro de utilização mais provável da API async. **Correção incorporada:** `Err` só quando nada foi proposto (`J-E0205`, `J-E0206`); `WriteOutcome::require_applied()?` para quem quer tratar a não aplicação como erro (`J-E0204`); o exemplo da documentação do SDK usa-o.

**S6 — Uma app 1.0 numa runtime 1.2 com um motivo de qualidade novo.** Pelo WIT, o adaptador não deixava passar o caso novo; pela imagem empacotada, o byte novo chegava à app sem nenhum tipo o impedir, e o SDK 1.0 não saberia o que fazer. *Quebrou.* **Correção:** cada caso novo tem uma projeção declarada, e o host projeta **antes** de escrever a imagem e os valores WIT, conforme o mundo da app (§2.8). Os `enum` Rust são `#[non_exhaustive]` para que o código-fonte também sobreviva a uma atualização do SDK.

**S7 — As reservas de ML presas à 1.0.** O documento 11 põe `ml` e `frames` no mundo da v1. Se fizessem parte do mundo estável 1.0, a sua forma ficava fixa para toda a série 1 (regra de ouro), antes de o subsistema de ML existir. *Quebrou.* **Correção:** `@unstable(feature = …)` no próprio pacote, fora do mundo estável; recusadas em `prod` (`J-E0003`), stubs em `dev`; estabilizadas na 1.1 com `@since` (§2.9). As escritas agendadas (M6) e as intenções (M18), que o documento 11 põe na 1.1, levam o mesmo tratamento.

**S8 — Um buffer de propostas malicioso.** Um guest pode devolver `n_records` falso, um `len` de método que sai do buffer, uma porta de entrada num `WRITE`, ou flags reservadas. **Aguenta:** validação estrutural completa antes de aplicar qualquer registo, e buffer inteiro descartado com trap (`J-E0907`), para que nunca haja uma aplicação parcial de uma saída atómica; erros de valor (NaN, enum inexistente) são `rejected(wrong-type)` só nessa proposta (§3.4).

**S9 — `HashMap` numa app Rust.** A `std` pede `wasi:random/insecure-seed`, e o componente não liga se o host não a fornecer, ou deixa de ser determinista se a fornecer com entropia real. *Quebrou* (o documento 04 não falava de WASI). **Correção:** o host fornece o conjunto WASI de §2.7, sempre virtualizado: aleatoriedade do gerador determinista, relógios do tick, sem ficheiros, rede só em async e com `net.allow`.

**S10 — Uma classe `pump@1.2` na app, uma instância `pump@1.0` no site.** Se os índices de membros viessem do contrato em runtime, o membro 5 da app seria outra coisa na instância. **Aguenta:** a tabela de portas é construída pela lista de membros **do handle**, gravada em `joker:meta`, e o membro que a instância não tem fica `bound = false` (§2.6.2).

**S11 — `h.output("led").last() == Some(false)` sem tipos.** Um traço genérico `OutputTrace<T>` exigiria anotação nos testes que só contam `toggles()`, e um traço de `Value` não se compara com `Option<bool>`. *Quebrou* com a primeira forma. **Correção:** traços de `Value`, com `Value: PartialEq<f64 | bool | i64 | u64>` e `Observed: PartialEq<Option<T>>` (§6.4): os testes da trilha compilam tal como estão.

**S12 — Desempenho de uma app de 120 VAVs.** 720 portas, das quais ~240 saídas: imagem de 64 + 24 × (720 + 240) ≈ 23 KB por tick, uma cópia pelo ABI canónico (~3 µs num A76 a 8 GB/s efetivos, a medir), e nenhuma chamada ao host. O resultado por saída custa 24 B, e é o que permite `last_outcome()` em 240 saídas sem 240 chamadas. **Aguenta**, dentro do orçamento do documento 09, ataque 2.

---

## 11. Decisões e alternativas rejeitadas

| Decisão | Alternativa rejeitada | Porquê |
|---|---|---|
| reservas da 1.1 com `@unstable` no mesmo pacote | pacote separado `joker:ml` com versão própria | quebraria a regra "uma versão para o mundo" do ref/00 §1 e obrigaria a uma matriz de versões; o `@unstable` é o mecanismo do próprio WIT e sai do mundo estável sem mudar nada |
| `cycle-result` com propostas e erro | `result<list<u8>, error>` | perde as propostas de uma tarefa com `Err` |
| sem alocação no `on-cycle` (trap) | alocador bump reposto por tick | ponteiros pendentes no estado, sem erro (S3) |
| porta = posição na tabela construída pela ordem de `joker:meta` | handles opacos dados pelo host | a tabela é reconstruível pelo SDK e pelo `jk inspect`, e os índices de membros não dependem do contrato em runtime (S10) |
| enums WIT com projeção declarada | códigos `u8` no WIT | o ref/00 fixa os enums WIT; a projeção dá tolerância sem perder os tipos |
| registos com chaves inteiras | chaves de texto | um `Internal<T>` de 3 campos cabia mal nos 64 B com nomes; o esquema está sempre disponível em `joker:meta` |
| `Many<C, MIN, MAX>` | `Many<C, {1..=8}>` | um `RangeInclusive` como parâmetro constante não é estável, e a macro teria de o reescrever num tipo que o utilizador não vê |
| `mins()` | `min()`, `minutes()` | `min` colide; `minutes` fica desalinhado dos outros sufixos curtos |
| registo das tarefas em tempo de ligação | `joker::export!(App, [control, replan])` | nenhum exemplo o tem, e esquecer uma tarefa na lista seria um erro silencioso |
| `joker:meta` como sequência de fragmentos | uma macro que conhece tudo | uma macro de atributo não vê os outros itens do crate |
| erros com `record error { code, message }` onde a app decide | variantes por interface (`action-error`) | um código de registo é extensível sem mudar o WIT, e é o que `jk explain` e a API usam; o `cancelled` das ações longas é o `J-E0308`, sem caso novo no WIT |
| um `run-task` por disparo, também em `on` e `on_event` | um `run-task` longo que consome `subscribe` no guest (proposta da ref/05) | o host conta e junta os disparos (§4.3.2) da mesma forma em todas as linguagens, e um guest que termina não deixa um gatilho mudo |
| plano B como mundo irmão `app-blocking` (§2.11) | funções bloqueantes no mundo `app` | o mundo `app` fica igual nas três linguagens; o plano B só existe se uma ADR o ativar |
| traços do harness sobre `Value` | traços genéricos | S11 |
| métodos como funções Wasm puras (14 §13) | — | continua alternativa futura; nada nesta API a impede |

---

## 12. Marcações de versão

| Item | Fase |
|---|---|
| `types`, `cycle-io` (exceto `write-at`), `async-io` (exceto `write-at` e `propose`), `kv`, `secrets`, `log`, `guest` | 1.0 |
| ABI de ciclo v1 (§3), exceto a flag `AT` | 1.0 |
| todo o SDK de §4, exceto §4.15 | 1.0 (backends `wasm` e `native-linux` na Fase 1; `native-micro` na Fase 2, documento 11) |
| instâncias virtuais, `#[implements]`, `on-action`, `on_action` | 1.0 (Fase 2) |
| `joker-test`, exceto `Series::replay` de `.jktrace` com ML | 1.0 (Fase 0 para o harness; Fase 1 para o componente) |
| `ml`, `frames`, `blocks`, `write-at`, `propose`, flag `AT`, `#[model]`, `Tensor` | **1.1**, reservados com `@unstable` |
| mundo `app-blocking` (plano B do tier 2, §2.11) | reservado; só com ADR na Fase 3 |
| Wasm no Joker Micro (`wasmi`) | 1.4 (documento 21): a ABI de ciclo não muda |
| mundo WIT 2 | 2.0 (ADR 0022) |
