# 02 — Modelo de sinais

O sinal é a única abstração que as apps veem. Tudo o que o Joker toca tem de caber nele, ou numa das suas três variantes (bloco, stream, evento), ou numa ação. O documento 09, ataque 1, testa onde esta abstração deixa passar detalhe.

Este documento explica o modelo. A semântica normativa (qualidade, expressões, árbitro, envelope) está no documento 16; a representação dos valores, da qualidade e dos resultados em cada interface (binário, JSON, CBOR) está na `ref/00` §4; as chaves do manifesto estão na `ref/01`.

---

## 1. As quatro formas de dados e as ações

| Forma | Semântica | Exemplo | Onde vive |
|---|---|---|---|
| **escalar** | último valor | `meter.p_total`, `di.door_open` | slot de 32 B + imagem de processo |
| **bloco** (M7) | sequência amostrada a ritmo fixo | `acc1.x` a 25,6 kS/s | anel de blocos por sinal |
| **stream** (M4) | frames volumosos, último ou fila | `cam0.frames` | pool de buffers partilhados |
| **evento** | registos discretos, fila, todos entregues | `vision.detections`, `plc.fault_event` | fila por consumidor, no historian |
| **ação** (M19) | comando com argumentos e resultado | `pcs.reset_fault()`, `opc.method("Calibrate")` | pedido/resposta pelo driver |

Regra de escolha: se o que interessa é *o valor agora*, é escalar. Se interessa *cada ocorrência*, é evento. Se é uma *forma de onda*, é bloco. Se é *grande*, é stream. Se é *fazer uma coisa* e não *pôr um valor*, é ação.

---

## 2. Tipos escalares

| Tipo | Notas |
|---|---|
| `bool` | |
| `i64`, `u64` | os drivers convertem de i16/u16/i32/u32 com escala |
| `f64` | tipo por omissão das grandezas físicas |
| `f32` | para poupar historian e uplink; convertido para f64 na leitura |
| `enum<Nome>` | valores nomeados declarados no perfil ou no manifesto (`Mode = { off = 0, standby = 1, run = 2, fault = 3 }`) |
| `bits<N>` | palavra de estado com bits nomeados (`status.bit(3)` ou `status.alarm_overtemp`) |
| `string` | ≤ 64 bytes UTF-8 (números de série, estados textuais) |
| `bytes` | ≤ 64 bytes |

Os sinais de dispositivos não têm `struct`. Grandezas relacionadas partilham um prefixo de caminho (`inv1.ac.p`, `inv1.ac.q`, `inv1.ac.f`), e a coerência entre elas é dada pela **coerência de aquisição** (§6.2). A única exceção são os **sinais internos de apps** (`Internal<T>`, documento 04, §3.4): podem ser registos de até 64 bytes só com escalares, guardados num slot largo com o mesmo protocolo seqlock.

Eventos e argumentos de ações usam **registos** com campos escalares, `string` até 1 KiB e listas limitadas (`list<T, 64>`). O esquema é declarado no código da app (`joker:meta`), no perfil ou no contrato da classe. Em binário, um registo é CBOR determinístico com as chaves inteiras iguais à posição do campo; em JSON, leva os nomes (`ref/00` §4.1).

---

## 3. Unidades

Cada sinal numérico tem uma unidade. Cada porta numérica de uma app também tem uma. A ligação entre as duas é verificada no `jk check`.

- **Sintaxe:** subconjunto do UCUM com os símbolos habituais da indústria (`W`, `kW`, `kWh`, `V`, `A`, `Hz`, `var`, `°C`, `K`, `bar`, `m3/h`, `%`, `1`, `gn`…). O catálogo normativo da v1, com prefixos e grandezas, está no documento 16, §5.
- **Dimensão:** cada unidade reduz-se a um vetor de dimensões SI mais um fator e um offset. `W` e `kW` têm a mesma dimensão, `W` e `Wh` não.
- **Conversão:** mesma dimensão → a runtime converte (fator, e offset para `°C`↔`K`). Dimensão diferente → erro `J0107`.
- **Casos especiais:**
  - `%` é adimensional mas **não** se liga automaticamente a `1`. É preciso declarar `bind.x = { signal = "…", as = "fraction" }`, porque a confusão 0–1 contra 0–100 é um bug clássico;
  - `var` e `W` têm a mesma dimensão física mas são grandezas diferentes: o Joker marca-as com *tipos de grandeza* distintos (`active_power`, `reactive_power`, `apparent_power`), e ligá-las entre si é `J0108`;
  - taxas: `kW/s` é a unidade de `rate` para uma saída em `kW`. O `jk check` verifica a coerência.
- **Literais com unidade** no manifesto e nas expressões: `"125 kW"`, `"49.8 Hz"`, `"20 kW/s"`, `"2s"`, `"5 %"`.

---

## 4. Qualidade

Herdada do OPC UA e simplificada para três níveis mais um motivo:

| Código | Motivos |
|---|---|
| `Good` | `None`, `Overridden` (valor forçado em simulação/teste), `LocalOverride` (o equipamento está em modo local) |
| `Uncertain` | `Stale`, `NotConfirmed` (readback divergente), `Substituted` (substituído por fallback declarado), `OutOfDistribution` (ML), `SensorCal` (em calibração), `ClockUnsynced`, `Initial` (ainda sem valor desde o arranque), `Frozen` (valor idêntico durante `stuck_detect`), `PartialWrite` (grupo atómico parcialmente escrito) |
| `Bad` | `CommFailure`, `DeviceFault`, `OutOfRange` (limites físicos declarados), `ConfigError`, `Stale`, `NotConnected`, `NaN`, `Tripped`, `Deadline` (pipeline ou modelo fora do prazo) |

**Sensor congelado.** Um transmissor avariado ou um PLC intermédio parado podem continuar a devolver o mesmo valor com `Good`. `stuck_detect = "30m"` num sinal (ou no perfil, para grandezas que variam sempre, como a frequência) marca `Uncertain(Frozen)` quando o valor fica bit-a-bit igual durante esse tempo.

**A tabela normativa de acesso** (`get`, `get_uncertain`, `get_or`, `sample`) e as regras de qualidade nas expressões estão no documento 16, §3. Os códigos numéricos e os nomes em minúsculas usados em JSON e TOML (`uncertain`, `comm_failure`) estão na `ref/00` §4.3.

**Invariante 3 do `AGENTS.md`:** uma amostra `Bad` não se converte num número. No SDK, `input.get()` devolve `Result<T, QualityError>`, e `input.sample()` devolve a amostra completa para quem quer decidir por si. `input.get_or(v)` existe e regista no traço que a leitura foi substituída; não altera a qualidade das saídas da app (documento 16, §3.1).

**Propagação nas expressões:**

| Operação | Regra |
|---|---|
| aritmética e comparação | pior qualidade dos operandos |
| `a && b`, `a \|\| b` | curto-circuito com qualidade, **simétrico**: `false && Bad` e `Bad && false` dão `false (Good)`; `true \|\| Bad` e `Bad \|\| true` dão `true (Good)`; nos outros casos, a pior. Os operandos não têm efeitos, e por isso o lado não importa |
| `any(sel)`, `all(sel)` | o mesmo curto-circuito, generalizado: um elemento `true (Good)` decide `any`, um `false (Good)` decide `all`; senão, a pior |
| `count`, `sum`, `maxs`, `mins` | pior qualidade dos elementos |
| permissivo (`interlock`) | resultado `Bad` ou `Uncertain` ⇒ **não permitido**, a não ser que `permit_uncertain = true` |
| alarme (`when`) | `Uncertain` avalia normalmente; `Bad` ⇒ o alarme fica em estado `Unknown` (16 §3.2) |
| funções de janela | a pior qualidade `q` tal que as amostras com qualidade igual ou pior do que `q` cobrem ≥ 10% da duração da janela (16 §3.2) |

---

## 5. Tempo e sequência

Cada amostra traz:

- `ts_src`: carimbo **UTC** do dispositivo remoto, se o protocolo o fornecer (OPC UA SourceTimestamp, BACnet, MQTT com timestamp);
- `ts_hw`: carimbo **monotónico** de captura em hardware local, dado pelo kernel (flanco de GPIO no chardev v2, buffer V4L2, tempo DC do EtherCAT traduzido). É a base certa para `write_at` (1.1), porque está no mesmo relógio do escalonamento. Num sinal que vem de um MCU pelo Joker Link, é o `ts_hw` do MCU convertido para o monotónico do nó pela estimativa do `TIME`, com a flag de domínio "link" (documento 01, §3.2; `ref/07` §8.3);
- `ts_acq`: instante **monotónico** em que o Joker a recebeu, com a tradução para UTC do documento 01, §18;
- `seq`: contador monotónico por sinal, que aumenta a cada amostra nova (mesmo que o valor não mude).

`cx.now()` numa tarefa de ciclo é o instante do **latch** do tick, e não o da chamada. Por isso duas tarefas do mesmo tick veem o mesmo "agora", e o replay é exato.

---

## 6. Semântica de leitura

### 6.1 Ciclo

Todas as tarefas de um tick leem do **snapshot do latch**: ler o mesmo sinal duas vezes no mesmo tick dá a mesma amostra. Os sinais escritos pelo próprio grupo mostram o valor **aplicado no tick anterior**, e não o proposto neste tick. Para ler o proposto, usa-se `output.pending()`.

### 6.2 Coerência de aquisição

Os sinais lidos pelo mesmo pedido de driver (um `read holding registers` que cobre `ac.p`, `ac.q` e `ac.f`, por exemplo) recebem o mesmo **id de aquisição**. `snapshot([…])` e o latch garantem que, se dois sinais têm o mesmo id de aquisição no momento da leitura, vêm do mesmo pedido. Dois sinais de pedidos diferentes nunca são garantidamente simultâneos. Se isso importa, o perfil deve juntá-los num único pedido (documento 05, §4).

### 6.3 Entre grupos

Um grupo lê de outro grupo o último valor aplicado **à data do seu latch**. Como os períodos são diferentes, não há alinhamento. Se uma tarefa de 10 ms e outra de 100 ms precisam de um valor coerente entre si, uma publica um sinal interno e a outra lê-o, sabendo que pode estar até um período atrasado.

### 6.4 Async

As tarefas async leem o slot atual, e `snapshot([…])` dá uma leitura coerente de vários slots (todos da mesma "época" seqlock). Não há latch: duas leituras seguidas podem diferir.

---

## 7. Semântica de escrita

### 7.1 Ciclo de vida de uma proposta

```
set(v) ──▶ staging do grupo ──▶ árbitro ──▶ envelope ──▶ commit ──▶ fila do driver ──▶ equipamento
                                   │            │                                      │
                                   ▼            ▼                                      ▼
                              Overridden   Clamped/Rejected/                     readback (opcional)
                                           Suppressed/Tripped                    → NotConfirmed
```

`WriteOutcome`, devolvido no tick seguinte ao ciclo (`output.last_outcome()`) e imediatamente num ticket async:

| Resultado | Significado |
|---|---|
| `Accepted` | aplicado tal como proposto |
| `Clamped(Range)`, `Clamped(Rate)` | aplicado com o valor limitado pela gama ou pela taxa; traz o valor aplicado. São dois resultados distintos (`clamped_range` e `clamped_rate`, `ref/00` §4.4) |
| `Suppressed` | dentro do deadband ou do intervalo mínimo; nada foi enviado |
| `Overridden(nível)` | a proposta ocupa o nível dela, mas um nível mais alto está ativo |
| `Deferred` | (async) à espera do próximo tick do grupo dono |
| `Shadowed` | a app está em modo sombra; gravado, não aplicado |
| `Rejected(motivo)` | `WrongType`, `Interlock`, `Quality`, `Range`, `AtomicGroup`, `NotOwner`, `ClockUnsynced`, `Budget` |
| `Tripped` | a saída entrou em trip por causa desta proposta |

Os valores numéricos e os nomes em JSON (`{"outcome": "clamped_rate", "applied": 1.0}`) estão na `ref/00` §4.4. `WrongType` em runtime quer dizer uma proposta NaN, infinita ou com uma variante de enum inexistente (16, E10). Numa saída de um MCU, o resultado vem do árbitro do MCU e chega por `OUTCOMES`, com um ou dois ticks de atraso (16, A12).

### 7.2 Escritas agendadas (M6, **1.1**)

**Reservado para a 1.1** (D5): as chaves `schedule_depth`, `timed`, `on_late` e `late_margin` existem no esquema, e usá-las numa runtime 1.0 é `J0290`; `write-at` é `@unstable` no WIT (`ref/04`); a API responde `J-E1017` a um pedido com `at`. O desenho fica aqui para que a 1.1 seja só uma mudança minor.

`output.write_at(v, t)` agenda uma escrita para o instante `t` (monotónico, normalmente derivado do `ts_hw` de uma entrada: "400 ms depois do flanco da fotocélula").

- A fila de escritas agendadas por saída é limitada (`schedule_depth`, por omissão 0: escritas agendadas desativadas; declara-se um valor, por exemplo 16, para as ativar) e ordenada por tempo.
- **Precisão:** por omissão, a escrita é aplicada no primeiro tick do grupo dono com `t_tick ≥ t`, o que dá uma precisão de um período. Com `timed = true` e um driver com saída temporizada (GPIO via thread `rt/drv` com `timerfd`, saídas de hardware temporizadas), a precisão é a do driver (alvo ±100 µs em GPIO com PREEMPT_RT).
- **`on_late`** (obrigatório quando `schedule_depth > 0`, `J0206`): se a proposta chega depois de `t − margin`, faz-se o que estiver declarado: `apply_now`, `drop`, ou `apply(v_fallback)`. Não há valor por omissão pela mesma razão que não há `safe` por omissão: o lado seguro depende do processo (documento 08, rodada 2).
- O envelope é avaliado **no instante da aplicação**, e não no do agendamento.

### 7.3 Grupos atómicos e ordem (M20)

Há equipamentos que exigem várias escritas coerentes: modo + setpoint, ou os registos "valor alto" e "valor baixo" de um u32, ou um "commit" após os parâmetros. O manifesto declara:

```toml
[atomic.pcs_cmd]
outputs = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
order   = ["pcs.mode", "pcs.p_set", "pcs.q_set"]   # ordem no barramento
commit  = { signal = "pcs.apply", value = 1 }      # opcional: escrita final de confirmação
```

- No envelope, ou todas passam ou nenhuma passa (passo 7). Com `on_rate = "proportional"`, se a taxa limitar uma das saídas a uma fração do passo pedido, todas as do grupo são limitadas à mesma fração (preserva relações como a curvatura de um veículo diferencial; documento 08, rodada 4).
- No driver, as escritas do grupo são enviadas pela ordem declarada, no mesmo pedido quando o protocolo o permite (Modbus FC16 contíguo), e sem outras escritas do mesmo dispositivo intercaladas.
- **Não há rollback no equipamento.** Se a terceira escrita falhar no barramento, as duas primeiras já lá estão. O grupo fica `Uncertain(PartialWrite)`, com alarme, e a próxima proposta repete o grupo inteiro. O documento 12, §5, declara esta fronteira.

---

## 8. Ações (M19)

Nem tudo é pôr um valor. Reset de falhas, arranque de calibração, métodos OPC UA, SDO de CANopen, rearme de proteções e atualização de firmware de um equipamento de campo são **comandos**.

```toml
[devices.pcs.actions.reset_fault]
permit  = "pcs.state == PcsState.fault"
role    = "operator"             # quem pode (app com permissão declarada ou humano)
timeout = "5s"
```

- A **implementação** (que registos se escrevem, por que ordem) é do perfil do dispositivo (`ref/03` §2.2) ou do contrato da classe; o manifesto só configura o permissivo, o papel e o timeout (`ref/01` §5.8.2).
- Uma ação tem argumentos tipados, um resultado (`Ok(record)` ou `Err(motivo)`) e um timeout.
- **Não é arbitrada**, porque não tem estado persistente que se possa disputar. É **serializada por dispositivo** e sujeita a um permissivo.
- **Idempotência:** cada invocação leva uma chave. Uma reinvocação com a mesma chave dentro de `dedup_window` devolve o resultado anterior em vez de repetir o comando.
- **Ações longas** (firmware, calibração) devolvem um `ActionHandle` com eventos de progresso e cancelamento (um cancelamento termina com o erro `cancelled`, `ref/04`).
- Só as tarefas **async** invocam ações, porque uma ação espera por resposta e o ciclo não espera. Uma tarefa de ciclo que precise de disparar uma ação publica um evento interno, e uma tarefa async executa-a.

---

## 9. Caminhos

```
caminho   := segmento ("." segmento)*
segmento  := [a-z][a-z0-9_]*          (máx. 32 caracteres; máx. 8 segmentos)
```

| Prefixo | Dono |
|---|---|
| `<dispositivo>.…` | driver do dispositivo declarado em `[devices.<dispositivo>]` |
| `app.<app>.…` | sinais publicados (derivados) e internos de uma app |
| `calc.<nome>` | sinais derivados por expressão no manifesto |
| `bridge.<ponte>.…` | valores recebidos por uma ponte (p. ex. `bridge.ros.cmd_v`), com `max_age` próprio |
| `<instância>.<membro>` | aliases dos membros de uma instância de classe (documento 16, P3) |
| caminhos de `to` de pipelines | declarados pelo pipeline (`vision.detections`, `p1.features`); únicos no site |
| `alarm.<nome>` | estado de um alarme |
| `joker.…` | a própria runtime (reservado) |

Os seletores no manifesto aceitam `*` para um segmento e `**` para vários: `"inv*.ac.p"`, `"meter.**"`. O `**` é sempre um segmento inteiro (`"alarm.**"`, e não `"**alarm.**"`). A gramática completa está na `ref/01` §3.9; os nomes e primeiros segmentos reservados, na `ref/00` §2.

---

## 10. Expressões

Uma linguagem pequena, tipada, com unidades, total (sem ciclos nem recursão) e avaliada pelo núcleo. É usada em permissivos, alarmes, derivados (`calc.*`), gatilhos de gravação e condições de reset.

```
expr    := or
or      := and ("||" and)*
and     := not ("&&" not)*
not     := "!" not | cmp
cmp     := sum (("=="|"!="|"<"|"<="|">"|">=") sum)?
sum     := prod (("+"|"-") prod)*
prod    := unary (("*"|"/") unary)*
unary   := "-" unary | atom
atom    := literal | caminho | call | "(" expr ")" | "if" expr "then" expr "else" expr
args    := expr | seletor                   (seletores com * e ** só como argumento de funções de agregação)
literal := número unidade? | "true" | "false" | Enum.variante | duração
call    := nome "(" (expr ("," expr)*)? ")"
```

**Funções:**

| Função | Tipo | Estado |
|---|---|---|
| `abs`, `min`, `max`, `clamp`, `sqrt`, `pow` | numéricas | — |
| `good(x)`, `quality(x)`, `age(x)` | meta | — |
| `avg(x, 10s)`, `minw(x, 1m)`, `maxw(x, 1m)`, `rate(x, 5s)` | janela | anel limitado; a janela máxima é declarada (`≤ 15m`) |
| `held(cond, 5s)` | a condição mantém-se há ≥ 5 s | 1 temporizador |
| `rose(cond)`, `fell(cond)` | flancos | 1 bit |
| `hyst(x, lo, hi)` | histerese | 1 bit |
| `bit(x, n)` | bits | — |
| `own(x)` | a última proposta do chamador, no seu nível, para o `out` `x` | só em métodos de classe (documento 17); fora deles, `J0306` |
| `any(sel)`, `all(sel)`, `count(sel)`, `sum(sel)`, `maxs(sel)`, `mins(sel)` | agregação sobre um seletor com `*`/`**` (`any(alarm.p1_*.active)`), resolvido no `jk check` para a lista fixa de sinais | — |

**Inteiros.** A aritmética faz-se em `f64`; as comparações entre dois inteiros do mesmo sinal são exatas (16 §4). **`own(x)`** sem proposta anterior do chamador devolve o valor aplicado de `x` (16 §4).

**Erros de compilação típicos:** dimensão incompatível (`meter.p > 50 Hz`, `J0301`), janela acima do limite (`J0305`), sinal desconhecido com sugestão (`J0302`), enum sem variante (`J0303`).

**Custo:** bytecode de pilha sobre `SignalId`, sem alocação, com o estado das janelas pré-alocado. Alvo: ≤ 50 ns por nó.

---

## 11. Alarmes (M9)

Um alarme é uma condição com ciclo de vida, na linha simplificada da ISA-18.2:

```
            condição verdadeira (após delay_on)                  ack
 Normal ─────────────────────────────────▶ Ativo·NãoReconhecido ──────▶ Ativo·Reconhecido
  ▲  ▲                                            │                            │
  │  │                                            │ condição falsa             │ condição falsa
  │  │                   ack                      ▼ (após delay_off)           │ (após delay_off)
  │  └─────────────────────────────────── RTN·NãoReconhecido                   │
  └────────────────────────────────────────────────────────────────────────────┘
```

Uma condição que volta a ser verdadeira em `RTN·NãoReconhecido` regressa a `Ativo·NãoReconhecido`.

```toml
[alarms.grid_overfreq]
when       = "meter.f > 50.2 Hz"
delay_on   = "2s"
delay_off  = "5s"
severity   = "high"            # low | medium | high | critical
suppress_when = "site.mode == SiteMode.maintenance"
message    = "Sobre-frequência na rede: {meter.f}"
actions    = { trip = ["pcs.p_set"] }   # opcional: trip de saídas
```

- **Shelving:** um operador pode arquivar temporariamente um alarme (`jk alarm shelve grid_overfreq 2h "manutenção"`), com TTL obrigatório e limitado por `max_shelve` (omissão 24 h, `ref/01` §5.15).
- **Supressão por desenho** (`suppress_when`): em manutenção ou em arranque, não se inunda o operador.
- Cada alarme é ao mesmo tempo **dois sinais**, `alarm.<nome>.state` (`enum` com os estados do diagrama, mais `Unknown`) e `alarm.<nome>.active` (`bool`, verdadeiro nos dois estados `Ativo·*`), e uma série de **eventos** (transições), gravados e enviados com a prioridade `alarms`.
- As apps também podem declarar alarmes (`#[alarm]`, documento 04), sujeitos às mesmas regras.

---

## 12. Classes e instâncias (M24)

Por cima dos sinais há uma camada opcional de **classes de equipamento**: contratos versionados (`led@1`, `pump@1`, `inverter.pv@2`) que agrupam membros (entradas, saídas, parâmetros, ações, eventos), métodos e consultas, e **instâncias** que ligam esses membros a sinais concretos. Tudo compila para os sinais deste documento: a semântica de leitura, escrita, qualidade e tempo é exatamente a mesma. O detalhe está no documento 14.
