# 17 — A IDL de classes (`.jkc`)

Documento normativo (ADR 0018). Define a linguagem em que se escrevem as **classes de equipamento** (documento 14): o que cada declaração significa, como corre em runtime, como se liga a equipamento real, o que o compilador gera e como evolui. Resulta da proposta `proposals/class-idl.md`, corrigida depois da rodada de prontidão (documento 18).

A regra que decide o formato de tudo no Joker (ADR 0018): **configuração é TOML; contratos são `.jkc`; comportamento é código.**

---

## 1. O que a IDL é, e o que não é

Uma classe `.jkc` descreve o **contrato de um tipo de equipamento para os programas**. É lida pelo compilador `jkc` (dentro da `jk`), pelo `jk check`, pelo núcleo (que executa métodos e consultas), pelo registo, pela HMI, pela ponte OPC UA e pela CLI.

A IDL **não** tem endereços nem protocolos (são dos perfis), segurança (é do manifesto; `@safe_hint` é só uma sugestão), ciclos, recursão, I/O nem estado próprio (são das implementações ativas). Tudo numa classe é analisável sem executar código.

---

## 2. As sete declarações

| Declaração | O que representa | Semântica em runtime | Produzida por | Consumida por | Ciclo / async |
|---|---|---|---|---|---|
| `in` | uma grandeza que o equipamento **tem agora** | último valor com qualidade e tempo | equipamento ou implementação | apps, consultas, alarmes | ambos |
| `out` | um **estado desejado** imposto ao equipamento | nível mantido por lease; árbitro; envelope; `safe` sem escritor | apps (por métodos ou direto), CLI, pontes | equipamento | ambos |
| `param` | uma **característica** da instância, que quase não muda | lida no arranque e a pedido; sem poll em ciclo | perfil, manifesto ou equipamento | apps, consultas | ambos |
| `event` | algo que **aconteceu** uma vez | fila; cada ocorrência entregue a cada consumidor | equipamento ou implementação | apps, historian, alarmes | async |
| `action` | um **pedido para o equipamento fazer uma coisa**, uma vez | pedido/resposta, timeout, idempotência, resultado; serializado; não arbitrado | apps, CLI, HMI, pontes | equipamento ou implementação | **só async** |
| `method` | açúcar para alterar um ou mais `out` de forma coerente | **expandido pelo núcleo** em propostas nos `out` | apps, CLI, HMI, pontes | núcleo | ambos |
| `query` | um valor **derivado** dos outros membros | expressão avaliada pelo núcleo em cada tick de quem a consome | núcleo | apps, alarmes | ambos |

### 2.1 `method` contra `action`

Um método roda um botão; uma ação carrega numa campainha.

| | `method` (`pump.start()`) | `action` (`pump.reset()`) |
|---|---|---|
| O que muda | um estado desejado que fica a valer | nada fica a valer: acontece uma coisa |
| Quem chamou desaparece | lease expira → `safe` | nada |
| Repetir | idempotente, se o método não usar `own()` (um `toggle()` repetido inverte duas vezes) | perigoso; chave de idempotência |
| Árbitro / envelope | sim / sim | não / permissivo + papel |
| Resposta | no ciclo, nenhuma; em async, `await` devolve os `WriteOutcome` das escritas expandidas, depois do commit | `Ok(resultado)` ou `Err(motivo)` |
| Custo | ~ns (vai no buffer de propostas) | um pedido ao driver: ms a s |

**Regras de decisão:**

1. Se, quando o chamador desaparece, o equipamento deve voltar a um estado seguro, é um `out` (com métodos por cima).
2. Se só faz sentido uma vez e tem um resultado, é uma `action`.
3. Se escrever tem custo físico (EEPROM, flash, desgaste), é uma `action` ou um `param`, e nunca um `out`.

---

## 3. A linguagem

### 3.1 Um ficheiro completo

```
// classes/std/pump.jkc
package std

/// Bomba com arranque/paragem e retorno de estado.
@aligns("CiA 402 quando o acionamento é um drive")
class pump@1.0.0 {

    /// Ordem de marcha.
    out run_cmd: bool            @safe_hint(false)

    /// Retorno de marcha (contactor ou drive).
    in  running: bool
    in  fault:   bool
    in  fault_code: u64          @optional

    param rated_flow: f64 m3/h   @optional

    event tripped { code: u64 }

    /// Rearma depois de uma falha.
    action reset() -> ()         @permit_hint(fault && !running) @timeout(5 s)

    method start()               { run_cmd = true }
    method stop()                { run_cmd = false }

    query failed_to_start: bool = held(run_cmd && !running, 10 s)
    query failed_to_stop:  bool = held(!run_cmd && running, 30 s)

    capability variable_speed {
        out speed_set: f64 %     @safe_hint(0 %) @range_hint(0 %, 100 %)
        in  speed:     f64 %
        atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
            speed_set = clamp(pct, 0 %, 100 %)
            run_cmd   = pct > 0 %
        }
    }

    capability pressure {
        in pressure_out: f64 bar
        query dry_running: bool = running && held(pressure_out < 0.2 bar, 5 s)
    }

    capability hour_counter {
        in hours: f64 h
    }

    health required(running, fault)
}
```

Uma bomba de velocidade fixa é uma instância **sem** a capacidade `variable_speed`: não há membros de saída que se tenham de ligar a constantes.

### 3.2 Gramática

```
file        := "package" pkgpath { use } { class | enum | record }
pkgpath     := ident { "/" ident }                       // std, acme/water
use         := "use" pkgpath "/" ident ("@" major)? ("as" ident)?
class       := doc* annot* "class" ident "@" semver "{" { item } "}"
item        := member | nested | capability | method | query | health | event | action
member      := doc* ("in" | "out" | "param") ident ":" type unit? ("=" literal)? annot*
nested      := doc* "member" ident ":" classref ("[" range "]")? annot*    // membro de classe (§8)
range       := n ".." n                                  // 0 ≤ min ≤ max ≤ 1024
capability  := "capability" ident "{" { member | nested | query | method | event | action } "}"
event       := doc* "event" ident ( record_body | ":" recordref )
action      := doc* "action" ident "(" params? ")" "->" rtype annot*
method      := doc* ("atomic")? "method" ident "(" params? ")" ("order" "(" idents ")")? "{" { stmt } "}"
stmt        := assign | call
assign      := ident "=" expr
call        := ident "." ident "(" ( expr { "," expr } )? ")"   // método de um membro de classe fixo (§4.1)
query       := doc* "query" ident ":" type unit? "=" expr
health      := "health" "required" "(" idents ")"
type        := "bool" | "i64" | "u64" | "f64" | "f32" | "string" | "bytes" | "bits" "<" n ">" | enumref
rtype       := type | recordref | "()" | "list" "<" (type | recordref) "," n ">"
param       := ident ":" (type unit? | recordref | "list" "<" … ">")
classref    := pkgpath "/" ident "@" major
annot       := "@" ident ("." ident)* ( "(" args ")" )?
expr        := a linguagem do documento 02, §10 (+ own(x) nos métodos; + m.x sobre um membro de classe fixo m;
               + agregações sobre membros coleção)
```

- **Versões:** a declaração tem a versão completa (`pump@1.0.0`); as referências usam a major (`std/pump@1`). O `joker.lock` fixa a versão exata e o hash do contrato.
- **Pacote implícito:** o pacote `std` pode ser omitido nas referências: `pump@1` ≡ `std/pump@1`. Qualquer outro pacote é sempre explícito.
- **Nomes:** pacotes com `/` (`std`, `acme/water`); classes podem ter pontos no nome (`hvac.vav`), porque o separador de pacote é `/`: `std/hvac.vav@1`.
- **Espaço de nomes dentro de uma classe:** membros, argumentos e consultas partilham-no. Um argumento com o nome de um membro é `JC0206`.
- **Nomes reservados:** `name`, `index`, `health`, `read`, `raw` e `has` são a API dos handles gerados em todas as linguagens, e não podem ser nomes de membros, consultas, métodos, eventos nem ações (`JC0207`). Um nome que seja palavra reservada de uma linguagem de destino (`from`, `in`, `class` em Python) é aceite, e o gerador dessa linguagem acrescenta-lhe `_` (`p.from_()`), com aviso.
- **Método contra membro:** um método pode ter o nome de um membro (`led@1`: `out on` e `method on()`). Nos handles gerados, o nome dá o **método**, e o membro fica em `raw()` (`led.raw().on`, ref/04 §4.8.2); na CLI, nas expressões e no manifesto, `led.on` é o membro.
- **Métodos:** só atribuições aos `out` da própria classe e chamadas a métodos de **membros de classe fixos** (`compressor.stop()`, §4.1). Não há `if` como instrução (há `if … then … else` como expressão), nem ciclos, nem chamadas a ações (`JC0401`), nem a métodos de um membro coleção (`JC0502`). Uma atribuição a um `out` de um membro (`compressor.run_cmd = true`) é `JC0204`: o membro muda-se pelos seus métodos. Cada `out` é atribuído no máximo uma vez por método, contando as escritas que as chamadas expandem (`JC0201`).
- **Membros de classe** (`member`, §8): fixos (`member tank: local/tank@1`) ou coleções (`member pumps: std/pump@1[1..8]`). Aceitam `@optional` e `@requires(<capacidade>, …)`, as capacidades que a instância ligada tem de ter (`JC0504` se a classe do membro não as tem). Podem estar dentro de uma `capability`. Nas expressões da classe, `m.x` lê o membro `x` do membro fixo `m`; uma coleção só se lê por agregações (§8).
- **Anotações:** as do Joker (`@safe_hint`, `@range_hint`, `@permit_hint`, `@role_hint`, `@timeout`, `@optional`, `@requires`, `@long`, `@aligns`, `@renamed_from`, `@deprecated`) são validadas. As de terceiros, com espaço de nomes (`@opcua.node(…)`, `@brick("Pump")`, `@hmi.label("Bomba")`), passam intactas para os metadados.

### 3.3 Enums e registos

```
package std

enum PcsMode { off = 0, standby = 1, charge = 2, discharge = 3 }

record SessionSummary {
    id:       string
    energy:   f64 kWh
    duration: f64 s
}
```

Os `in`/`out`/`param` são sempre escalares ou enums (documento 02, §2). Os registos e as listas limitadas (`list<T, N>`, N ≤ 64) só aparecem em `event` e em argumentos ou resultados de `action`.

---

## 4. Modelo de execução

### 4.1 Método

1. A app chama `pump.run_at(60 %)`. O SDK acrescenta ao **buffer de propostas** do tick um **registo de método**: `(instância, método, argumentos)`, com os argumentos em CBOR (ref/04 §3.3).
2. No passo 5a do tick (documento 01, §4), o núcleo expande os registos **pela ordem das chamadas**: avalia as atribuições no snapshot do tick e com `own()` = última proposta do chamador no seu nível, incluindo as anteriores do mesmo tick. **Sem proposta anterior** (primeira chamada, ou lease expirado), `own(x)` é o valor aplicado de `x` (documento 16, §4).
3. As atribuições tornam-se propostas normais, com grupo atómico e ordem se o método for `atomic`, e seguem para o árbitro e o envelope.

Consequências:

- O mesmo método é chamado de qualquer lado pelo mesmo caminho: `jk call p1.run_at 60% --ttl 10m`, um método OPC UA vindo do SCADA, uma intenção aprovada. A expansão acontece no próximo tick do grupo dono da saída, no nível do árbitro de quem chama.
- Duas chamadas a `toggle()` no mesmo tick anulam-se.
- No tick em que chamou o método, a app não conhece os valores expandidos (`pending()` desses `out` devolve `None`). Conhece-os no tick seguinte, com `last_outcome()`, ou em async esperando pelo `await`.

**Métodos que chamam métodos dos membros.** Um método de uma classe composta pode chamar um método de um **membro fixo**:

```
class compressor@1.0.0 {
    member drive: std/drive@1
    in discharge_temp: f64 °C

    method start(pct: f64 %) { drive.run_at(pct) }
    method stop()            { drive.stop() }
}
```

1. A expansão é a do passo 2, recursiva: o núcleo resolve `drive` na instância composta (a instância ligada a esse membro, ref/01 §5.13), avalia os argumentos no snapshot do tick e expande o método chamado sobre essa instância, **no mesmo passo 5a**. A árvore de chamadas inteira expande-se de uma vez; não há um tick por nível.
2. As escritas expandidas são propostas **do escritor que chamou o método de topo**, no seu nível: `own()` dentro de `drive.stop()` é o do chamador de `comp1.stop()`. Para o árbitro de `vfd1.run_cmd`, não há diferença entre chamar `comp1.stop()` e chamar `vfd1.stop()` diretamente (documento 16, A1 e P6).
3. Num método `atomic`, as escritas expandidas pelas chamadas entram no mesmo grupo atómico. O `order` pode nomear o membro (`order(drive, bypass)`), e as escritas dele ocupam essa posição, pela ordem do método chamado.
4. A profundidade das chamadas é limitada pela da composição (§8): no máximo 3 classes em cadeia, e nunca há recursão, porque uma classe não se pode conter a si mesma.
5. Chamar um método de um membro coleção (`pumps.stop()`), ou iterar, é `JC0502`: comportamento sobre coleções vai para uma app ou para uma implementação ativa. Chamar um método que o membro não tem, ou de uma capacidade que o membro não exige com `@requires`, é `JC0503`. Chamar uma ação é `JC0401`.
6. **Numa instância virtual**, os membros de classe são da implementação, e quem usa a instância não escreve neles (documento 16, P6). Uma instância virtual de uma classe cujos métodos chamam métodos dos membros é `J0656` no `jk check`: a chamada faria de quem chama um segundo escritor dos membros, ao lado da implementação. Uma classe que se quer usar como virtual escreve só nos seus `out`, e a implementação decide o que fazer aos membros.

### 4.2 Ação

`invoke(instância.ação, args, chave)` → permissivo e papel (do manifesto) → deduplicação pela chave → fila serializada do dispositivo → passos do perfil (ou a tarefa `on-action` da implementação virtual) → `Ok`/`Err`. Uma ação `@long` devolve um `ActionHandle` com eventos de progresso e cancelamento.

**Configuração das ações:** a classe dá sugestões (`@timeout`, `@permit_hint`, `@role_hint`). O manifesto decide, em `[devices.<d>.actions.<a>]` (instâncias implícitas) ou em `[instances.<i>.actions.<a>]` (compostas e virtuais), com as chaves `permit`, `role` e `timeout`. O manifesto vence sempre, e um `@timeout` da classe só é usado quando o manifesto não o declara.

### 4.3 Consulta

Compilada para o bytecode das expressões e avaliada no latch de cada grupo que a consome. Estado volátil (documento 16, §3.2).

### 4.4 Evento

Produzido pelo driver, por um adaptador `on_rise`/`on_change` do perfil, ou pela implementação. Fila limitada por consumidor; gravado se a telemetria o pedir.

### 4.5 Saúde

`health required(a, b)`: com um dos obrigatórios não `Good`, a instância está `Failed`; com um membro de uma capacidade declarada não `Good`, está `Degraded`; senão, `Ok`.

**Uma instância composta** (com membros de classe, §8) tem a saúde **pior** (`Failed` > `Degraded` > `Ok`) entre a da regra acima, sobre os seus membros escalares, e a dos seus membros de classe:

| Membro de classe | Conta como |
|---|---|
| obrigatório (fora de capacidades, sem `@optional`) | a saúde da instância ligada, tal e qual; numa coleção, a pior dos elementos |
| de uma capacidade que a instância declara | no máximo `Degraded`: um `Failed` do membro dá `Degraded` |
| de uma capacidade que a instância não declara | não conta (não está ligado) |
| `@optional` | só se estiver ligado, e no máximo `Degraded` |

A regra é recursiva: a saúde da instância ligada já inclui a dos seus membros. O `Degraded(m)` e o `Failed(m)` dos handles (ref/04 §4.8.2) levam o nome do membro de classe (`compressor`), e o `GET /v1/instances/{i}` mostra a árvore com a saúde de cada nó (ref/09 §5.11). O `health required(…)` só nomeia membros escalares; os de classe contam pela tabela.

**Uma instância implícita com várias classes.** Um perfil com vários `[implements]` (um VFD que é `pump@1` e `meter.ac3@1`) dá **uma** instância implícita por dispositivo (ref/03 §2.2.14) que cumpre todas essas classes. A sua saúde é a **pior** das saúdes calculadas pela regra de cada classe que cumpre (`Failed` > `Degraded` > `Ok`), cada uma sobre os membros mapeados para essa classe. O `jk status --instances` e o `GET /v1/instances/{i}` mostram a saúde agregada e, por classe, a de cada regra, para se ver qual delas a baixou. Um handle de uma dessas classes numa app (`io.p.health()`) devolve a saúde agregada, e não a da sua classe: é o mesmo equipamento.

---

## 5. Ligação a equipamento real

### 5.1 Perfil (instância implícita)

```toml
# profiles/acme/vfd@3.profile.toml (excerto)
[implements."std/pump@1"]
capabilities = ["variable_speed", "hour_counter"]
map = { run_cmd = "cmd.run", running = "status.running", fault = "status.fault", fault_code = "fault_code", speed_set = { signal = "speed_ref", scale = 0.01 }, speed = "speed_act", hours = "counters.run_hours", reset = "actions.reset_fault", tripped = { on_rise = "status.fault", fields = { code = "fault_code" } } }
```

### 5.2 Adaptadores

Os valores do `map` são um caminho, ou um adaptador. Todos são declarativos e avaliados pelo núcleo.

| Adaptador | Para quê | Exemplo |
|---|---|---|
| `scale`, `offset`, `invert` | valores e lógica invertida | `{ signal = "ref", scale = 0.01 }` |
| `const:` | **só em `in` e `param`**: um membro que o equipamento não mede (`J0603` num `out`) | `"const:true"` |
| `edge` | um `out` de nível sobre um equipamento que só aceita impulsos | `{ edge = { rise = "actions.start", fall = "actions.stop" } }` |
| passos de ação | uma `action` sobre registos | passos do perfil (documento 05, §6) |
| `on_rise`, `on_change` | um `event` a partir de um sinal | `{ on_rise = "status.fault" }` |
| `enum_map` | enums com números diferentes | `{ signal = "op_mode", enum_map = { off = 0, standby = 3, charge = 1, discharge = 2 } }` |

**O adaptador `edge`, em detalhe.** É o que permite que uma bomba com botões de marcha e paragem por impulso continue a ter a semântica segura de um `out`:

- dispara a ação `rise` quando o valor **aplicado** passa de falso a verdadeiro, e `fall` quando passa de verdadeiro a falso;
- **no arranque de cada geração**, executa uma vez a ação do nível `safe` (sincronização), para o equipamento ficar num estado conhecido;
- o keepalive (documento 01, §8) **não** se aplica a membros com `edge`, e o `jk check` exige que o equipamento tenha o seu próprio fallback de comunicação (`J0205` em `prod`);
- o bit `force_safe` do supervisor (documento 01, §3.4) é honrado pelo driver, que executa a ação `fall` diretamente, sem passar pelo grupo.

### 5.3 Instâncias compostas e virtuais

```toml
[instances.p1]
class = "std/pump@1"
capabilities = ["pressure"]
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }

[instances.p2]
class = "std/pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_3", feedback = "io.di_3", thermal_trip = "io.di_4" } }

[instances.p2.actions.reset]
role = "operator"
```

- `implemented_by = "app:<nome>"` usa o `bind` de `[apps.<nome>]` e serve **uma** instância. A forma em tabela (`{ app, bind }`) cria **uma instância da app por instância virtual**, cada uma com as suas ligações. É assim que três bombas partilham a mesma implementação.
- A implementação recebe os `out` como comandos (`io.cmd.*`), publica os `in` (`io.state.*`), emite os `event` e trata as `action` com o export `on-action` (ref/04 §2.6 e §4.8.4).
- Numa instância composta, o valor de um membro de classe no `map` é o nome de uma instância (ou uma lista, numa coleção); numa instância virtual, o `map` pode existir ao lado do `implemented_by`, só com os membros de classe (ref/01 §5.13, §8).

---

## 6. O que o `jkc` gera

```
classes/std/pump.jkc
  │ jkc check    parse → pacotes → tipos e unidades → regras (§7)
  ▼
pump@1.0.0.contract.json    forma canónica, ordenada, sem comentários
  ├─ hash de contrato   (membros, tipos, métodos, consultas, capacidades) → joker.lock
  └─ hash de documentação (docs, anotações de HMI)                        → não parte compatibilidade
  ├─▶ Rust    crate joker-classes-std   handles CycleIo/TaskIo, enums, registos, métodos (emitem registos)
  ├─▶ TS      @joker/classes-std
  ├─▶ Python  joker_classes.std
  ├─▶ docs    página para o registo e para a HMI
  └─▶ OPC UA  ObjectType + Methods para a ponte (M11)
```

Os `.contract.json` vão para o dispositivo dentro de cada geração (com o manifesto e o lock), e o registo de instâncias do núcleo é construído a partir deles.

---

## 7. Regras do `jkc check`

| Código | Regra |
|---|---|
| `JC0107` | grandeza física sem unidade |
| `JC0201` | método atribui a um `out` mais do que uma vez |
| `JC0204` | método atribui a algo que não é `out` |
| `JC0205` | `atomic` sem `order`, ou `order` com membros não atribuídos |
| `JC0206` | argumento com o mesmo nome de um membro |
| `JC0207` | nome reservado (`name`, `index`, `health`, `read`, `raw`, `has`) num membro, consulta, método, evento ou ação |
| `JC0210` | uso de um membro de uma capacidade fora dela |
| `JC0301` | expressão com dimensões incompatíveis |
| `JC0310` | consultas em ciclo |
| `JC0320` | consulta com janela acima de 15 min |
| `JC0401` | ação usada num método (também a de um membro: `compressor.reset()`) |
| `JC0402` | ação sem `@timeout` |
| `JC0501` | composição com profundidade > 3 (contada como no §8) |
| `JC0502` | método que chama um método de um membro coleção, ou que itera sobre ele |
| `JC0503` | chamada a um método que o membro não tem, ou de uma capacidade que o membro não exige com `@requires` |
| `JC0504` | `@requires` com uma capacidade que a classe do membro não tem |
| `JC0601` | versão incompatível com a anterior sem subir a major (§9) |
| `JW0230` | método condicionado por um `in` (parece um interlock; confirma que o manifesto também o tem) |

**Dois limites de 3, que não se somam.** O `JC0501` é do `jkc` e conta a **composição de classes** (§8). O `J0620` é do `jk check` e conta as **camadas de implementação**: instâncias virtuais cuja app usa outras instâncias virtuais (documento 14, §7.2). Uma `heat_pump@1` com profundidade 3, implementada por uma app que usa um compressor virtual, tem profundidade 3 e 2 camadas, e passa nos dois.

---

## 8. Composição e coleções

```
class station@1.0.0 {
    member pumps: std/pump@1[1..8]
    member tank:  local/tank@1          // a std/ não tem tanques (14 §10)
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
    query pumps_running:   u64  = count(pumps.running)
}
```

As agregações (`any`, `all`, `count`, `sum`, `maxs`, `mins`) são a única forma de "iterar" numa classe. Um método de uma classe composta não itera (`stop_all()` não é expressável, `JC0502`): comportamento sobre coleções vai para uma app ou para uma implementação ativa. Sobre um membro fixo, um método pode chamar os métodos dele (§4.1).

**Profundidade.** Conta-se pela cadeia mais longa de membros de classe a partir da classe, contando a própria classe como 1: `station` = 1, o `pump` de `pumps` = 2, um `drive` dentro da bomba = 3. Uma classe com uma cadeia de 4 é `JC0501`; uma classe que se contém a si mesma, direta ou indiretamente, também, porque a cadeia não acaba. As camadas de implementação contam-se à parte (§7, `J0620`).

**O resto das regras dos membros de classe está onde a regra vive:**

| Assunto | Regra | Onde |
|---|---|---|
| ligar um membro numa instância composta | o valor no `map` é o nome de uma instância; numa coleção, uma lista de nomes | ref/01 §5.13 (`J0650`–`J0654`) |
| caminhos | `<instância>.<membro>.<submembro>`; numa coleção, com o nome da instância do elemento (`station.pumps.p1.running`) | documento 16, P6 |
| uma instância em várias composições | permitido; um só envelope e um só árbitro por `out` | documento 16, P6 (`J0655`) |
| métodos que chamam métodos dos membros | só membros fixos | §4.1 |
| saúde | a pior entre a regra da classe e a dos membros | §4.5 |
| um perfil que cumpre uma classe composta | subtabelas `members.<membro>` | ref/03 §2.2.14 |
| handles gerados | `io.esp.boot.clicked()`, `io.station.pumps.iter()` | ref/04 §4.8.6, ref/05 §14.6 |
| num Joker Micro | o catálogo leva a árvore | ref/07 §6.3 |

---

## 9. Versões

| Mudança | Classificação |
|---|---|
| acrescentar capacidade, membro `@optional`, consulta, método, evento, ação | minor |
| documentação ou anotações de HMI | patch (não muda o hash de contrato) |
| unidade para outra da mesma dimensão (`W` → `kW`) | minor (a runtime converte) |
| dimensão, tipo, direção (`in` ↔ `out`), apagar ou renomear um membro | major |
| renomear com `@renamed_from("old")` | minor, com alias durante uma major |
| tornar obrigatório um membro opcional | major |
| mudar o corpo de um método ou de uma consulta | major |
| acrescentar um campo **no fim** de um `record` ou de um `event` | minor |
| reordenar, remover ou inserir a meio um campo de um registo | major |

Os registos (eventos, argumentos e resultados de ações e métodos) viajam em CBOR como um mapa com **chaves inteiras = posição do campo** no contrato (ref/00 §4.1, D2 do documento 22). É por isso que, dentro de uma major, os campos só se acrescentam no fim: um leitor antigo ignora as chaves que não conhece, e um campo novo não pode ocupar a posição de outro.

O `jk class diff <a> <b>` aplica esta tabela e é executado pelo `jk class publish`.

---

## 10. Porque não WIT, OPC UA NodeSet, Protobuf ou IEC 61131

| | `.jkc` | WIT | OPC UA NodeSet | Protobuf/gRPC | IEC 61131 FB |
|---|---|---|---|---|---|
| unidades e grandezas verificadas | sim | não | declaradas, não verificadas | não | não |
| estado desejado com lease e `safe` | sim (`out`) | não | não | não | parcial |
| pedido/resposta | sim (`action`) | sim | sim | sim | não |
| eventos | sim | sim | sim | streams | não |
| derivados avaliados pelo sistema | sim (`query`) | não | não | não | em código |
| capacidades opcionais | sim | não | parcial | não | não |
| analisável sem executar | sim | sim | sim | sim | não |

Nenhum junta unidades, estado desejado seguro, pedido/resposta e derivados. A ponte OPC UA gera um ObjectType a partir de cada classe, e por isso o `.jkc` não compete com o OPC UA: alimenta-o.

---

## 11. Casos de estresse que definiram a linguagem

Os 15 casos da proposta original (arranque por impulsos, reset, bateria com modo e limites dinâmicos, fabricantes de inversores, carregador EV, firmware, EEPROM, `toggle` duplo, SCADA a chamar métodos, `stop_all`, métodos condicionados, leitura de um `out`, versões misturadas, crescimento da linguagem) estão em `proposals/class-idl.md`, §10. As correções que a rodada de prontidão fez sobre eles estão no documento 18, §2.

**Leitura de um `out`:** dá o valor **aplicado**. `x.mine()` dá a proposta do próprio escritor, `x.winner()` o nível que ganhou. Nas expressões da classe, `x` é o aplicado e `own(x)` é o do chamador.
