# Proposta — A IDL de classes (`.jkc`)

**Estado:** incorporada no **documento 17** (ADR 0018). Este ficheiro fica como registo do raciocínio e dos 15 casos de estresse (§10). Onde divergir do documento 17, **vale o 17** (a sintaxe foi corrigida na rodada de prontidão, documento 18).

O objetivo é responder a três perguntas: o que é exatamente cada coisa que se declara numa classe (e porque é que `method` e `action` são coisas diferentes); como é que cada uma corre em runtime; e onde é que a IDL parte quando se lhe atira equipamento real.

---

## 1. O que a IDL é, e o que não é

Uma classe `.jkc` descreve **o contrato de um tipo de equipamento para os programas**. É lida por:

- o compilador `jkc` (dentro da `jk`), que valida e gera os tipos para Rust, TypeScript e Python;
- o `jk check`, que verifica ligações, capacidades, unidades e grupos atómicos;
- o núcleo, que executa os métodos e as consultas;
- o registo, a HMI, a ponte OPC UA e a CLI, que mostram as instâncias e deixam chamar métodos e ações.

A IDL **não** tem: endereços nem protocolos (isso é dos perfis), segurança (isso é do manifesto: `safe_hint` é só uma sugestão), ciclos, recursão, I/O nem estado próprio (isso é das implementações ativas). Tudo o que se escreve numa classe tem de ser analisável sem executar código.

---

## 2. As sete coisas que uma classe declara

Esta é a parte central. Cada tipo de membro corresponde a **um comportamento em runtime diferente**, e escolher o errado é o erro de modelação mais caro.

| Declaração | O que representa | Semântica em runtime | Quem a produz | Quem a consome | Onde pode ser usada |
|---|---|---|---|---|---|
| `in` | uma grandeza que o equipamento **tem agora** | último valor com qualidade e tempo; snapshot no ciclo | o equipamento (driver) ou a implementação | apps, consultas, alarmes | ciclo e async |
| `out` | um **estado desejado** que alguém impõe ao equipamento | nível mantido por um lease; árbitro; envelope; `safe` quando ninguém o mantém | apps (por métodos ou diretamente) | o equipamento | ciclo e async |
| `param` | uma **característica** da instância que quase não muda | lido no arranque e a pedido; sem poll em ciclo | o perfil, o manifesto ou o equipamento | apps, consultas | ciclo e async |
| `event` | algo que **aconteceu**, uma vez | fila; cada ocorrência entregue a cada consumidor | o equipamento ou a implementação | apps, historian, alarmes | async |
| `action` | um **pedido para o equipamento fazer uma coisa**, uma vez | pedido/resposta, com timeout, idempotência, resultado ou erro; não é arbitrado | apps, CLI, HMI, pontes | o equipamento (driver) ou a implementação | **só async** |
| `method` | **açúcar** para alterar um ou mais `out` de forma coerente | expandido pelo núcleo em propostas nos `out`; sem resposta própria | apps, CLI, HMI, pontes | o núcleo | ciclo e async |
| `query` | um valor **derivado** dos outros membros | expressão avaliada pelo núcleo em cada tick de quem a consome | o núcleo | apps, alarmes | ciclo e async |

### 2.1 `method` contra `action`: a diferença que interessa

A melhor imagem é esta: **um método roda um botão; uma ação carrega numa campainha.**

| | `method` (`pump.start()`) | `action` (`pump.reset()`) |
|---|---|---|
| O que muda | um **estado desejado** (`run_cmd = true`) que fica a valer | nada fica a valer: **acontece uma coisa** no equipamento |
| Se quem chamou desaparecer | o lease expira e o `out` volta ao `safe`: a bomba para | nada: o reset já foi feito (ou falhou) |
| Repetir a chamada | idempotente: pedir `true` duas vezes é o mesmo que uma | perigoso: dois resets são dois resets, e por isso há chave de idempotência |
| Árbitro | sim: operador vs. otimizador, prioridades, overrides | não: não há estado a disputar; é **serializado** por equipamento |
| Envelope | sim: gama, taxa, interlocks, orçamento de atuações | um **permissivo** (`permit`) e um papel (`role`) |
| Resposta | não tem; o resultado vê-se no `out` e no `last_outcome` do tick seguinte | tem: `Ok(resultado)` ou `Err(motivo)`, com timeout |
| Onde se pode chamar | ciclo e async (é só uma proposta) | só async (espera por uma resposta) |
| Custo | ~ns: vai no buffer de propostas do tick | um pedido ao driver: ms a s |
| Exemplo | `start`, `stop`, `set_power(50 kW)`, `toggle`, `open`, `set_mode(auto)` | `reset_fault`, `acknowledge`, `calibrate`, `start_session`, `update_firmware`, `save_config` |

**A regra de decisão**, para quem escreve a classe:

> Se, quando o chamador desaparece, o equipamento deve **voltar a um estado seguro**, é um `out` (e um `method` por cima dele). Se o que se pede **só faz sentido uma vez** e tem um resultado, é uma `action`.

Os casos difíceis estão nos ataques do §10 (S2, S6, S8), e são exatamente os que obrigam a esta distinção.

### 2.2 Porque é que o `method` não é um `out` "com nome bonito"

Porque um método pode escrever **vários** `out`, de forma **atómica** e **ordenada**, com valores **calculados** dos argumentos e do estado. `battery.set_power(-30 kW)` escreve o modo (`discharge`) e o setpoint (`30 kW`) no mesmo tick, pela ordem que o equipamento exige, e nunca se vê um tick com o modo novo e o setpoint velho. É um contrato que o equipamento precisa e que quem usa a classe não devia ter de conhecer.

---

## 3. A linguagem

### 3.1 Um ficheiro completo

```
// classes/std/pump@1.jkc
package std

/// Bomba com arranque/paragem, velocidade variável e retorno de estado.
@aligns("CiA 402 quando o acionamento é um drive")
class pump@1 {

    // ── estado desejado ──────────────────────────────────────────
    /// Ordem de marcha.
    out run_cmd:   bool          @safe_hint(false)
    /// Referência de velocidade.
    out speed_set: f64 %         @safe_hint(0 %) @range_hint(0 %, 100 %)

    // ── estado atual ─────────────────────────────────────────────
    /// Retorno de marcha (contactor ou drive).
    in  running:   bool
    in  speed:     f64 %
    in  fault:     bool
    in  fault_code: u64          @optional

    // ── características ──────────────────────────────────────────
    param rated_flow: f64 m3/h   @optional

    // ── acontecimentos ───────────────────────────────────────────
    event tripped { code: u64, at_speed: f64 % }

    // ── pedidos ao equipamento ───────────────────────────────────
    /// Rearma a bomba depois de uma falha. Só é permitido com a bomba parada e em falha.
    action reset() -> () @permit_hint(fault && !running) @timeout(5 s)

    // ── açúcar sobre os out ──────────────────────────────────────
    method start()               { run_cmd = true }
    method stop()                { run_cmd = false; speed_set = 0 % }
    atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
        speed_set = clamp(pct, 0 %, 100 %)
        run_cmd   = pct > 0 %
    }

    // ── derivados ────────────────────────────────────────────────
    query failed_to_start: bool = held(run_cmd && !running, 10 s)
    query failed_to_stop:  bool = held(!run_cmd && running, 30 s)

    // ── capacidades opcionais ────────────────────────────────────
    capability pressure {
        in pressure_out: f64 bar
        query dry_running: bool = running && held(pressure_out < 0.2 bar, 5 s)
    }

    capability hour_counter {
        in hours: f64 h
    }

    health {
        required running, fault
    }
}
```

### 3.2 Gramática (resumo)

```
file        := "package" ident { use } { class | enum | record }
use         := "use" path ("as" ident)?
class       := doc* annot* "class" ident "@" major "{" { item } "}"
item        := member | capability | method | query | health | event | action | nested
member      := doc* ("in" | "out" | "param") ident ":" type unit? annot*
nested      := doc* "member" ident ":" classref ("[" range "]")? annot*     // composição (§8)
capability  := "capability" ident "{" { member | query | method | event | action } "}"
event       := "event" ident record_body
action      := doc* "action" ident "(" params? ")" "->" type annot*
method      := doc* ("atomic")? "method" ident "(" params? ")" ("order" "(" idents ")")? "{" { assign } "}"
assign      := ident "=" expr                         // um out por atribuição; no máximo uma vez por método
query       := doc* "query" ident ":" type unit? "=" expr
health      := "health" "{" "required" idents { "degraded_if_bad" idents } "}"
type        := "bool" | "i64" | "u64" | "f64" | "f32" | "string" | "bytes" | enumref | "bits" "<" n ">"
unit        := literal de unidade (documento 02, §3), opcionalmente com "as" <grandeza>
annot       := "@" ident ( "(" args ")" )?
expr        := a linguagem de expressões do documento 02, §10, sem seletores
```

Três escolhas de sintaxe deliberadas:

- **As expressões são as do manifesto.** O mesmo parser e o mesmo bytecode: o que funciona num `calc` funciona numa `query`.
- **Os métodos só têm atribuições.** Não há `if` como instrução (há `if … then … else` como expressão), nem ciclos, nem chamadas a outros métodos. Um método é uma função pura `(argumentos, snapshot, próprias propostas) → atribuições`.
- **As anotações são abertas mas com espaço de nomes.** As do Joker (`@safe_hint`, `@optional`, `@timeout`…) são validadas. As de terceiros (`@opcua.node(…)`, `@brick("Pump")`, `@hmi.label("Bomba")`) passam intactas para os metadados, e é por aí que se faz o alinhamento com normas e a HMI.

### 3.3 Tipos, unidades e registos

```
package std

enum PcsMode { off = 0, standby = 1, charge = 2, discharge = 3 }

record SessionSummary {
    energy:   f64 kWh
    duration: f64 s
    id:       string
}
```

- Os `enum` e os `record` pertencem ao pacote e são referidos por nome (`std.PcsMode`).
- Os registos só aparecem em `event` e em argumentos ou resultados de `action`. Um `in`/`out` é sempre escalar (documento 02, §2).
- As unidades são obrigatórias em tudo o que é `f64`/`f32` com significado físico, e `1` para adimensional. Um número sem unidade numa classe é `JC0107`.

---

## 4. Como corre: o modelo de execução

### 4.1 Um método

```
 app (Wasm, ciclo)                 núcleo (mesmo tick)                           equipamento
 ─────────────────                 ───────────────────                           ───────────
 pump.run_at(60 %)
   └─▶ registo no buffer de propostas:  { instância p1, método run_at, args [60 %] }
                                   passo 5a: expandir métodos, pela ordem das chamadas
                                     speed_set = clamp(60 %, 0, 100) = 60 %
                                     run_cmd   = 60 % > 0 % = true
                                     → 2 propostas, grupo atómico, ordem (speed_set, run_cmd)
                                   passo 5b: árbitro (nível do chamador)
                                   passo 6:  envelope (do manifesto)
                                   passo 7:  commit ─────────────────────────────▶ FC16 contíguo
```

- O método **não atravessa a fronteira Wasm como uma chamada**: vai no mesmo buffer de propostas da imagem empacotada (documento 09, ataque 2). O custo é o de avaliar a expressão (~50 ns por nó).
- `own(x)` é a última proposta **do chamador, no seu nível do árbitro**, para o `out` `x`, incluindo as propostas anteriores do mesmo tick. Duas chamadas a `toggle()` no mesmo tick anulam-se, como se espera.
- Como o método é expandido pelo núcleo, **o mesmo método pode ser chamado de qualquer lado**: `jk call p1.run_at 60% --ttl 10m` na CLI, um método OPC UA vindo do SCADA, uma intenção aprovada. Todos passam pelo mesmo código, no nível do árbitro de quem chama.
- **Limitação assumida:** dentro do tick em que chamou o método, a app não sabe os valores expandidos (`pending()` desse `out` devolve `None`). Sabe-os no tick seguinte, com `last_outcome()`.

### 4.2 Uma ação

```
 app (Wasm, async)                          núcleo / driver                         equipamento
 ─────────────────                          ───────────────                         ───────────
 pump.reset().await
   └─▶ invoke(p1.reset, args, chave) ──▶  permissivo (manifesto) ✓
                                          papel do chamador ✓
                                          dedup pela chave ✓
                                          fila do dispositivo (serializada) ──────▶ passos do perfil:
                                                                                     write 40300 = 1
                                                                                     wait 500 ms
                                                                                     write 40300 = 0
   ◀── Ok(()) ou Err(Timeout | Denied | Device(msg)) ◀──────────────────────────────
```

- Numa instância **virtual**, a ação é tratada por uma tarefa async da implementação (`#[joker::action] async fn reset(...)`).
- Uma ação longa devolve um `ActionHandle` com eventos de progresso e cancelamento (documento 02, §8).

### 4.3 Uma consulta

Compilada para bytecode e avaliada **no latch de cada grupo** que a consome, como um sinal derivado (documento 14, §8, A7). Estado volátil: janelas ≤ 15 min, flancos e temporizadores recomeçam num reinício.

### 4.4 Um evento

Produzido pelo driver (um evento OPC UA, uma palavra de alarme que muda de bit) ou pela implementação (`io.state.tripped.emit(…)`). Entregue a cada consumidor numa fila limitada e gravado se a telemetria o pedir.

---

## 5. Como uma classe se liga a coisas reais

A classe diz **o quê**. O **como** é dito por um de três implementadores, e é aí (e não na classe) que vivem as diferenças entre equipamentos.

### 5.1 Um perfil (instância implícita)

> **Obsoleto.** A forma abaixo (`[implements."std.pump@1"]`, com os membros diretamente na tabela) não é a adotada. A forma normativa é `[implements."std/pump@1"]` com os membros dentro de `map`, como no manifesto: documento 17, §5.1, e `ref/03` §2.2 (achado A11 da `ref/03`). O bloco fica como registo do raciocínio.

```toml
# profiles/acme/vfd@3.profile.toml
[implements."std.pump@1"]
run_cmd    = "cmd.run"                                       # nível → nível
speed_set  = { signal = "speed_ref", scale = 0.01 }          # adaptador de escala
running    = "status.running"
speed      = "speed_act"
fault      = "status.fault"
fault_code = "fault_code"
reset      = "actions.reset_fault"                            # ação → passos do perfil
tripped    = { on_rise = "status.fault", fields = { code = "fault_code", at_speed = "speed_act" } }
capabilities = ["hour_counter"]
hours      = "counters.run_hours"
```

### 5.2 Adaptadores

Os adaptadores são a peça que absorve as diferenças entre equipamentos sem as levar para a classe. São **declarativos** e avaliados pelo núcleo, como tudo o resto:

| Adaptador | Para quê | Exemplo |
|---|---|---|
| `scale`, `offset`, `invert` | valores e lógica invertida | `speed_set = { signal = "ref", scale = 0.01 }` |
| `const` | membro que o equipamento não tem | `speed = "const:100 %"` (bomba de velocidade fixa) |
| `edge` | **um `out` de nível sobre um equipamento que só aceita impulsos** | `run_cmd = { edge = { rise = "actions.start", fall = "actions.stop" } }` |
| `level` | uma `action` sobre um equipamento que só tem um registo | passos do perfil (`write 1`, `wait`, `write 0`) |
| `on_rise`/`on_change` | um `event` a partir de um sinal | `tripped = { on_rise = "status.fault" }` |
| `enum_map` | enums com números diferentes | `mode = { signal = "op_mode", map = { off = 0, standby = 3, charge = 1, discharge = 2 } }` |

### 5.3 O manifesto (instância composta) e uma app (instância virtual)

São os mesmos mapeamentos, no `[instances.*]` do manifesto (documento 14, §5.2), ou implementados em Rust por uma app com `#[joker::implements(Pump)]`, que recebe os `out` como comandos (`io.cmd.*`), publica os `in` (`io.state.*`), trata as `action` com tarefas async e emite os `event`.

---

## 6. O que o `jkc` gera

```
classes/std/pump@1.jkc
   │ jkc check        parse → resolver pacotes → tipos e unidades → regras (§7)
   ▼
pump@1.contract.json  forma canónica, ordenada, sem comentários nem formatação
   │                  ├─ hash de contrato  (membros, tipos, métodos, consultas) → vai para o joker.lock
   │                  └─ hash de documentação (docs, anotações de HMI)          → não parte compatibilidade
   ├─▶ Rust    joker-classes-std::Pump   handles CycleIo/TaskIo, enums, records, métodos que emitem registos
   ├─▶ TS      @joker/classes-std        tipos e métodos
   ├─▶ Python  joker.classes.std         idem
   ├─▶ docs    página HTML/Markdown      para o registo e para a HMI
   └─▶ OPC UA  ObjectType + Methods      para a ponte (M11)
```

**A identidade de uma classe é o hash do contrato canónico, e não o texto.** Reformatar, comentar ou traduzir a documentação não muda o hash de contrato, e nenhum `joker.lock` fica desatualizado por isso.

Os erros usam o prefixo `JC` e o formato da `jk`:

```
error[JC0204]: método escreve num membro que não é `out`
  --> classes/std/pump@1.jkc:34:9
   |
34 |         running = true
   |         ^^^^^^^ `running` é `in`: é o equipamento que o diz, não a app
   = ajuda: se querias pedir marcha, é `run_cmd = true`
```

---

## 7. As regras que o `jkc check` aplica

| Código | Regra |
|---|---|
| `JC0107` | grandeza física sem unidade |
| `JC0201` | método atribui a um `out` mais do que uma vez |
| `JC0204` | método atribui a algo que não é `out` |
| `JC0205` | `atomic` sem `order`, ou `order` com membros não atribuídos |
| `JC0210` | método ou consulta usa um membro de uma capacidade sem estar dentro dela |
| `JC0301` | expressão com dimensões incompatíveis |
| `JC0310` | consultas em ciclo (`a` usa `b` que usa `a`) |
| `JC0320` | consulta com janela acima de 15 min |
| `JC0401` | ação usada num método (os métodos não esperam) |
| `JC0402` | ação sem `@timeout` |
| `JC0501` | composição com profundidade > 3 |
| `JC0601` | nova versão incompatível com a anterior sem subir a major (§9) |

---

## 8. Composição e coleções

```
class station@1 {
    member pumps: std.pump@1[1..8]
    member tank:  std.tank@1
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
    query pumps_running:   u64  = count(pumps.running)
}
```

- `any`, `all`, `count`, `sum`, `maxs`, `mins` sobre um membro de uma coleção são as agregações do documento 02, §10, e são a única forma de "iterar" numa classe.
- **Um método de uma classe composta não pode iterar** sobre os membros (`stop_all()` não é expressável). É de propósito: comportamento sobre coleções é lógica, e vai para uma app ou para uma implementação ativa da `station@1`.

---

## 9. Versões

O `jk class diff pump@1.jkc@1.2 pump@1.jkc@1.3` classifica cada mudança:

| Mudança | Classificação |
|---|---|
| acrescentar uma capacidade, um membro `@optional`, uma consulta, um método, um evento, uma ação | minor |
| mudar a documentação ou anotações de HMI | patch (não muda o hash de contrato) |
| mudar a unidade para outra da mesma dimensão (`W` → `kW`) | minor (a runtime converte) |
| mudar a dimensão, o tipo, a direção (`in` ↔ `out`), apagar ou renomear um membro | **major** |
| renomear com `@renamed_from("old")` | minor, com alias durante uma major |
| tornar obrigatório um membro que era opcional | **major** |
| mudar o corpo de um método ou de uma consulta | **major**, a não ser que o `jk class diff` prove equivalência (raro); regra prática: mudar a semântica é major |

---

## 10. Estresse: 15 casos contra a IDL

**S1 — LED.** Três linhas de `out`, quatro métodos. Aguenta; é o caso trivial e tem de continuar trivial.

**S2 — Bomba que só aceita impulsos de arranque e paragem.** Muitos arrancadores têm registos "start" e "stop" que se escrevem com 1 (e o equipamento guarda o estado), em vez de um bit de marcha. Se a classe tivesse `action start()`, perdia-se a propriedade mais importante: **quando o chamador desaparece, a bomba para**. Decisão: a classe continua com `out run_cmd` (nível) e o **perfil** adapta com `edge = { rise = "actions.start", fall = "actions.stop" }`. Quando o lease expira e o `run_cmd` vai para `safe`, o adaptador envia o impulso de paragem. A diferença entre equipamentos fica no perfil, e a semântica segura fica na classe. É o caso que justifica a regra do §2.1.

**S3 — Reset.** É uma `action`: não há estado desejado ("estar rearmado" não é algo que se mantenha), a repetição é perigosa e há um resultado (o equipamento pode recusar). Com um lease, um reset seria repetido a cada tick. Aguenta, com a regra.

**S4 — Bateria com modo e setpoint, e limites dinâmicos.** `atomic method set_power(p: f64 kW) order(mode, p_set) { mode = if p < 0 kW then PcsMode.discharge else if p > 0 kW then PcsMode.charge else PcsMode.standby; p_set = abs(p) }`. O modo e o valor chegam juntos. Mas o limite de carga muda com a temperatura das células (`in p_max_charge`), e a classe não o pode impor, porque a segurança é do site. **Achado:** o envelope do manifesto precisa de aceitar **gamas dinâmicas**, com expressões (`range = ["-bess.p_max_discharge", "bess.p_max_charge"]`). A classe pode sugerir a relação com `@range_hint(-p_max_discharge, p_max_charge)`, e o `jk check` mostra a sugestão. É uma alteração pedida ao documento 03 (§12).

**S5 — Cinco fabricantes de inversores.** Núcleo pequeno (`p_ac`, `p_limit_pct`, `state`) e capacidades (`reactive`, `ramp`, `mppt_detail`). Um fabricante sem limitação por percentagem, só em W: o perfil adapta com `scale` a partir de um `param rated_power`. Aguenta.

**S6 — Carregador de veículos elétricos.** O caso que usa todas as declarações:

```
class evse@1 {
    in  state: EvseState                   // idle, connected, charging, fault
    in  current: f64 A
    out current_limit: f64 A   @safe_hint(6 A)   // estado desejado: se o gestor morrer, 6 A
    event session_ended SessionSummary
    action start_session(id_tag: string) -> SessionId   @timeout(10 s)
    action stop_session(id: SessionId) -> SessionSummary @timeout(10 s)
    method limit(a: f64 A) { current_limit = a }
}
```

O limite de corrente é um `out`, porque tem de voltar a um valor seguro se o gestor de carga desaparecer. Iniciar e terminar uma sessão são `action`, porque acontecem uma vez, têm uma resposta (o id, o resumo) e não devem ser repetidas. O fim da sessão também é um `event`, para quem não o pediu (a faturação). **Aguenta, e mostra que as três coisas são mesmo diferentes.**

**S7 — Atualização de firmware.** `action update_firmware(image: ArtifactRef) -> () @timeout(30 min) @long`. Uma ação longa com progresso. Não é um método, e muito menos um `out`. Aguenta.

**S8 — Parâmetro guardado em EEPROM.** Um variador tem a "velocidade por omissão" num parâmetro que é gravado em memória não volátil, com ~100 000 ciclos de escrita. Se fosse um `out`, os keepalives (documento 01, §8) e as escritas a cada mudança gastavam a EEPROM em semanas. Decisão: **configuração persistente do equipamento é uma `action`** (`action set_default_speed(pct: f64 %)`) ou um `param`, e nunca um `out`. (A proposta previa um `jk device configure` para escrever `param`; **não foi adotado**. Na v1, a configuração persistente faz-se por uma `action` do perfil, ou pela `config` do dispositivo que o perfil aplica no `connect`, `ref/03` §2.2.10. Não há comando nem endpoint que escreva um `param`, `ref/09` §9.) Regra acrescentada ao §2.1: *se escrever custa desgaste, não é um `out`*.

**S9 — `toggle()` duas vezes no mesmo tick.** A expansão é sequencial e `own()` vê as propostas anteriores do tick: o resultado é o valor inicial. É o comportamento esperado, e fica num caso de conformidade.

**S10 — O SCADA chama um método.** A ponte OPC UA gera um `ObjectType` por classe, e cada `method` e `action` vira um método OPC UA. Uma chamada a `run_at(60)` vinda do SCADA é expandida pelo núcleo **no nível do árbitro da ponte** (documento 03, §12). O SCADA não consegue fazer nada que um operador na CLI não pudesse, e passa pelo mesmo envelope. Aguenta, e é o argumento definitivo contra métodos em código de app.

**S11 — `station.stop_all()`.** Não é expressável (§8). A tentação de acrescentar ciclos à IDL é forte, e deve ser recusada: seria o primeiro passo para uma linguagem de programação dentro da IDL. O caminho é uma implementação ativa da `station@1`, ou uma app.

**S12 — Um método que precisa de ler uma entrada.** `method safe_start() { run_cmd = !fault }`: a expressão lê um `in`, o que é permitido (lê do snapshot de quem chama). Mas "arrancar só se não houver falha" é **segurança**, e pertence ao interlock do manifesto. O `jkc` aceita, e emite um aviso (`JW0230: um método condicionado por um `in` parece um interlock; confirma que o manifesto também o tem`).

**S13 — Ler um `out`.** O que é `p1.run_cmd` para quem lê? Há três valores: a minha última proposta, o valor efetivo (quem ganhou o árbitro) e o valor aplicado (depois do envelope). Decisão: ler um `out` dá o **aplicado** (é o que o equipamento recebeu). `p1.run_cmd.mine()` dá a minha proposta, e `p1.run_cmd.winner()` dá o nível que ganhou. Nas expressões da classe, `run_cmd` é o aplicado e `own(run_cmd)` é o do chamador.

**S14 — Duas versões da mesma classe num site.** Um perfil antigo implementa `pump@1.2` e uma app pede `pump@1.4` com uma capacidade nova. O `jk check` compara a capacidade pedida com as declaradas pela instância (`J0602`) e não com a versão minor: uma instância `1.2` que não tem a capacidade falha, e uma que tem, passa. Aguenta.

**S15 — A IDL cresce até ser uma linguagem.** É o risco de fundo. Contenção: a gramática do §3.2 é fechada e cabe numa página; qualquer construção nova exige ADR; e o teste de que uma proposta pertence à IDL é sempre o mesmo: *o núcleo precisa de a executar ou analisar para quem não é uma app Rust?* Se não, vai para uma extension trait ou para uma implementação ativa.

---

## 11. Comparação com o que já existe

| | Joker `.jkc` | WIT | OPC UA NodeSet | Protobuf/gRPC | IEC 61131 FB |
|---|---|---|---|---|---|
| Unidades e grandezas | sim, verificadas | não | sim (EUInformation), não verificadas | não | não |
| Estado desejado com lease e `safe` | sim (`out`) | não | não | não | parcial (saídas) |
| Pedido/resposta | sim (`action`) | sim | sim (Methods) | sim | não |
| Eventos | sim | sim (streams) | sim | streams | não |
| Derivados avaliados pelo sistema | sim (`query`) | não | não | não | sim (código) |
| Capacidades opcionais | sim | não (world composition) | parcial (optional) | não | não |
| Analisável sem executar | sim | sim | sim | sim | não |

Nenhum dos existentes junta **unidades + estado desejado seguro + pedido/resposta + derivados**. É isso que justifica uma IDL própria em vez de WIT com anotações: seria preciso anotar quase tudo, e as anotações não seriam verificadas.

---

## 12. O que esta proposta pede aos outros documentos (se for aceite)

1. **Documento 14:** trocar os blocos TOML das classes por `.jkc`; acrescentar `param`, `event` e a tabela do §2 como definição oficial dos membros; a regra do §2.1.
2. **Documento 03:** gamas de envelope com expressões (S4); ~~`jk device configure` para parâmetros persistentes (S8)~~ (não adotado; ver S8).
3. **Documento 05:** os adaptadores do §5.2 nas secções `implements` dos perfis, incluindo `edge` (S2).
4. **Documento 07:** `jk class fmt | check | diff | doc | gen`, `jk call <instância>.<método>`.
5. **Documento 15:** `classes/**/*.jkc` e `*.contract.json` gerados.
6. **Trilha, parte 8:** `status_light@1.jkc`.
7. **ADR 0018:** "um formato por tipo de coisa: TOML para configuração, `.jkc` para contratos, código para comportamento".

## 13. Como validar antes de decidir

- Escrever as ~12 classes da `std/` (documento 14, §10) em `.jkc` e em TOML, lado a lado.
- Pedir a duas pessoas (uma de automação, uma de software) para escreverem `valve.motor@1` e `evse@1` só com esta proposta.
- Implementar o `jkc check` para a gramática do §3.2 (estimativa: 1–2 semanas, reutilizando o `joker-expr`).
- Aceitar se: a DX for claramente melhor; nenhum dos casos S1–S15 precisar de mais do que a gramática; e a forma canónica gerada for idêntica nas duas versões (o que prova que o formato de superfície é só superfície).
