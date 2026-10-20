# 14 — Classes de equipamento, instâncias e camadas de abstração

A pergunta que originou este documento: *"Gosto de `led: Output<bool>`, mas queria poder escrever `led: Led` e fazer `led.toggle()`. E numa app com 15 equipamentos, cada um com 5 sinais e ações, uma struct com 75 portas fica estranha."*

Resposta curta: **sim, com uma condição.** As classes são uma camada **por cima** dos sinais, e não em vez deles. Compilam para portas e sinais, e por isso o envelope, o árbitro, a qualidade, o historian e o replay continuam a funcionar sem mudar nada. Ninguém é obrigado a usá-las, e quem quiser continua a descer até ao sinal cru.

O objetivo é servir bem **dois públicos** ao mesmo tempo: quem quer acesso de baixo nível para **construir abstrações**, e quem só quer **usar abstrações** para escrever lógica de negócio.

Este documento define a primitiva **M24** (documento 10) e a ADR 0017, e ataca o desenho com 14 casos (§8).

---

## 1. Três camadas, quatro pessoas

```
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ CAMADA 3 — LÓGICA DE NEGÓCIO                                               │
 │   station.pumps.iter().filter(|p| p.failed_to_start()) …                   │
 │   usa classes; nunca vê registos, pinos, protocolos                        │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ CAMADA 2 — CLASSES E INSTÂNCIAS                                            │
 │   led@1, button@1, analog_in@1, pump@1, meter.ac3@1, inverter.pv@2 …       │
 │   instâncias implícitas (perfil), compostas (manifesto) e virtuais (app)   │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ CAMADA 1 — SINAIS E PORTAS (documento 02)                                  │
 │   Input<f64>, Output<bool>, Action, eventos, blocos, streams               │
 ├────────────────────────────────────────────────────────────────────────────┤
 │ NÚCLEO — imagem de processo, árbitro, envelope, qualidade, traço           │
 └────────────────────────────────────────────────────────────────────────────┘
```

| Pessoa | Trabalha em | Escreve | Não precisa de saber |
|---|---|---|---|
| **Integrador de hardware** | camada 1 → 2 | perfis de equipamento com `implements` (declarativo, TOML) | a lógica de negócio |
| **Autor de abstrações** | camada 2 | classes, implementações ativas em Wasm, testes de conformidade da classe | que equipamento está por baixo |
| **Programador de negócio** | camada 3 | apps que usam classes | registos, pinos, protocolos, escalas |
| **Operador** | todas, a ler | nada: vê instâncias por classe no `jk status` e na HMI | — |

A mesma pessoa pode fazer as três coisas, e na trilha de estudo fez. O que interessa é que **cada camada é completa**: dá para ficar só numa delas e produzir algo útil.

---

## 2. Conceitos

| Conceito | O que é | Exemplo |
|---|---|---|
| **Classe** | contrato versionado do que um tipo de equipamento **é para um programa**: membros, capacidades, métodos, consultas | `led@1`, `pump@1`, `inverter.pv@2` |
| **Membro** | um elemento da classe: `input`, `output`, `param`, `action`, `event`, `query` | `pump.running` (input), `pump.run_cmd` (output) |
| **Capacidade** | grupo opcional de membros; uma instância tem-na ou não | `led` com `dimmable`; `pump` com `pressure` |
| **Método** | açúcar sobre escritas em membros de saída, definido na classe | `led.toggle()`, `pump.run_at(60 %)` |
| **Consulta** (*query*) | valor derivado dos membros, calculado **pelo núcleo** | `button.clicked` = `rose(pressed)` |
| **Instância** | um equipamento concreto que cumpre uma classe, com os membros ligados a sinais | `p1: pump@1` |

**Uma classe descreve o papel, não o chip.** O ESP32 da trilha é um *dispositivo* (fala Modbus RTU com o firmware de bancada e tem um perfil, `local/bench-esp32@1`). O seu sensor de toque, o pad T0, é uma *instância* de `analog_in@1`: a implícita do perfil, `esp`, com a escala nominal do firmware, e a composta `toque` do site, com a calibração daquela bancada (trilha, parte 7, §5). A app quer "um valor analógico normalizado", e não "o pad T0 de um ESP32". A regra prática: se trocar o componente por outro equivalente não devia mexer na app, então o componente é um dispositivo, e o papel é uma classe.

A mesma regra vale para as classes compostas. `local/bench_io@1` (trilha, parte 8, "Extra") é aceitável: descreve **o nó de I/O da bancada**, com um LED de estado, um botão, um toque e um sinal de vida, e outro nó com as mesmas peças cumpria-a sem mexer na app. Uma classe chamada `esp32@1` é o erro: diz o chip, e não o papel, e no dia em que o nó passar a ser um RP2350 a classe mente. O nome de uma classe responde a "para que serve isto no programa?", e nunca a "que peça é esta?".

---

## 3. Camada 1: continua igual, e é o escape

Nada do documento 02 muda. `Input<f64>`, `Output<bool>`, as ligações por caminho, `get()?`, `sample()`, `write_at`, as ações e os eventos continuam a ser a base. Duas garantias:

1. **Uma classe é sempre expansível para portas.** `jk inspect` mostra a lista plana de portas que uma app com classes realmente usa, e é essa lista que o host Wasm, o árbitro e o traço veem.
2. **Todo o handle de classe dá acesso aos membros crus:** `io.led.raw().on` é um handle de saída (`CycleOutput<bool>` no ciclo). Quem precisar de uma operação de porta num membro que a classe não expõe como método, tem-na. Quando um método tem o nome de um membro (em `led@1`, o `out on` e o `method on()`), o handle dá o método, e o membro fica em `raw()` (documento 17, §3.2).

---

## 4. Camada 2: definir uma classe

As classes vivem num **registo de classes** versionado (como os perfis, documento 05, §6). Há uma biblioteca padrão `std/` e classes do projeto em `classes/`. Escrevem-se na IDL **`.jkc`** (documento 17, ADR 0018), e o `jkc` gera a partir delas os tipos para Rust, TypeScript e Python. Assim, as três linguagens veem exatamente os mesmos membros e métodos. O documento 17 é a referência da linguagem; esta secção mostra as classes da trilha e do resto deste documento.

### 4.1 `std/led@1`

```
package std

/// Indicador luminoso.
class led@1.0.0 {
    out on: bool                       @safe_hint(false)

    method on()           { on = true }
    method off()          { on = false }
    method set(v: bool)   { on = v }       // JC0206 se o argumento se chamasse `on`
    method toggle()       { on = !own(on) } // ver §8, A6; o nome `on` dá o método, o membro fica em raw()

    capability dimmable {
        out brightness: f64 %          @safe_hint(0 %)
        method dim(pct: f64 %) { brightness = pct; on = pct > 0 % }
    }
}
```

### 4.2 `std/button@1`

```
package std

/// Botão de pressão momentâneo.
class button@1.0.0 {
    in pressed: bool

    query clicked:   bool = rose(pressed)          // verdadeiro durante um único tick
    query released:  bool = fell(pressed)
    query long_hold: bool = held(pressed, 1 s)
}
```

### 4.3 `std/analog_in@1`

```
package std

/// Entrada analógica normalizada (0..1), com o valor físico disponível.
class analog_in@1.0.0 {
    in value: f64 1                    // a normalização é do perfil ou da instância

    query smooth: f64 1 = avg(value, 200 ms)

    capability physical {
        in reading: f64                // valor físico; unidade dada pela instância (V, mA…)
    }
}
```

O membro físico chama-se `reading`, e não `raw`: `raw` é um nome reservado dos handles gerados (`io.led.raw()` dá os membros crus; `JC0207`, documento 17 §3.2).

### 4.4 `std/pump@1`: um equipamento a sério

A definição completa está no documento 17, §3.1. O essencial: `out run_cmd`, `in running`, `in fault`, a ação `reset`, os métodos `start`/`stop`, as consultas `failed_to_start`/`failed_to_stop`, e as capacidades `variable_speed` (com `speed_set`, `speed` e o método atómico `run_at`), `pressure` e `hour_counter`. Uma bomba de velocidade fixa é uma instância sem `variable_speed`.

### 4.5 As regras de uma classe

1. **Não há código livre.** Os métodos só atribuem valores a membros `out` e chamam métodos de membros fixos (ADR 0027), com expressões da linguagem do documento 02, §10, mais `own(x)`. As consultas são expressões avaliadas **pelo núcleo** (§8, A7). Quem precisar de comportamento a sério escreve uma **implementação ativa** (§7) ou uma extension trait (§6.7).
2. **Não há herança.** Há composição (§4.6) e capacidades. A herança em modelos de equipamento produz as hierarquias que nenhum integrador consegue manter.
3. **Uma classe não define segurança.** `@safe_hint` é uma sugestão. O `safe`, a gama, a taxa e os interlocks continuam no manifesto do site (ADR 0006), e o `jk check` mostra a sugestão da classe ao lado do erro `J0201`.
4. **Versões semver** com as regras do documento 17, §9. As apps dependem da major (`std/pump@1`); o lock fixa a versão exata e o hash do contrato.

### 4.6 Composição

```
package local

class station@1.0.0 {
    member pumps: std/pump@1[1..8]
    member tank:  local/tank@1          // a std/ não tem tanques (§10): é uma classe do projeto
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
}
```

A profundidade máxima de composição é 3 (`JC0501`). Conta-se pela cadeia mais longa de membros de classe, contando a própria classe como 1: `station` = 1, `pump` = 2, o `drive` de uma bomba = 3 (documento 17, §8). Mais do que isso é quase sempre um modelo de ativos (que pertence a um sistema de gestão de ativos) disfarçado de controlo. As camadas de implementação (uma instância virtual cuja app usa outras instâncias virtuais) contam-se à parte, com o mesmo limite de 3 (`J0620`, §7.2). Os dois limites não se somam.

Usar uma classe composta, de ponta a ponta:

| Passo | Como | Onde está a regra |
|---|---|---|
| ligar os membros | no `map`, o valor de um membro de classe é o nome de uma instância; numa coleção, uma lista de nomes | §5.2; ref/01 §5.13 |
| chegar aos sinais | caminhos aninhados: `esp_io.boot.clicked` é o mesmo sinal que `botao.clicked`; numa coleção, `station.pumps.p1.running` | documento 16, P6 |
| partilhar um membro | uma instância pode ser membro de várias composições; um só envelope e um só árbitro por `out`; aviso `J0655` quando a partilha chega a apps diferentes | documento 16, P6 |
| chamar métodos dos membros | num método da classe, só de membros fixos (`drive.stop()`) | documento 17, §4.1 |
| saúde | a pior entre a da classe e a dos membros obrigatórios | documento 17, §4.5 |
| um só equipamento que é a composição inteira | o perfil cumpre a classe com subtabelas `members.*` | ref/03 §2.2.14 |
| no SDK | `io.esp.boot.clicked()`, `io.station.pumps.iter()` | ref/04 §4.8.6; ref/05 §14.6 |

O caso completo, com três níveis, uma instância virtual e um Joker Micro, está no §9.3.

---

## 5. Instâncias: três maneiras de nascer

### 5.1 Implícitas: o perfil diz que o equipamento cumpre a classe

Para o caso comum de um equipamento por instância. O perfil declara o mapeamento, e **cada dispositivo com esse perfil passa a ser uma instância**, com o nome do dispositivo.

```toml
# profiles/acme/vfd@3.profile.toml (excerto)
[implements."std/pump@1"]
capabilities = ["variable_speed"]
map = { run_cmd = "cmd.run", speed_set = "speed_ref", running = "status.running", speed = "speed_act", fault = "status.fault", reset = "actions.reset_fault" }
```

```toml
# joker.toml
[devices.vfd1]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 11
profile = "acme/vfd@3"          # vfd1 é, automaticamente, uma instância de pump@1
```

### 5.2 Compostas: o equipamento lógico espalha-se por vários dispositivos

Na indústria, **o que se controla raramente é um dispositivo de comunicação**. Uma bomba real é um contactor numa saída digital, um retorno numa entrada digital, uma relé térmica noutra, e um transmissor de pressão 4–20 mA num cartão analógico. Três dispositivos, um equipamento.

```toml
[instances.p1]
class = "std/pump@1"
capabilities = ["pressure"]                                   # bomba de velocidade fixa: sem variable_speed
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }
```

- Os membros passam a ter também um caminho na instância (`p1.run_cmd` é um alias de `io.do_1`). O envelope declara-se por **um** dos dois caminhos, nunca pelos dois (`J0215`, documento 16, P2).
- `const:` liga um membro `in` ou `param` a uma constante, com qualidade `Good`. Num `out` é `J0603`: um equipamento que não tem uma saída não tem a capacidade que a contém.
- Os adaptadores disponíveis (`scale`, `invert`, `edge`, `enum_map`, `on_rise`…) estão no documento 17, §5.2.
- Membros sem mapeamento: obrigatórios → `J0601`; de uma capacidade declarada → `J0602`.
- **Membros de classe.** O valor é o nome de uma instância, e numa coleção uma lista de nomes, pela ordem da coleção. A instância pode nascer de qualquer maneira (implícita, composta, virtual ou simulada):

```toml
[instances.station]
class = "local/station@1"
map   = { pumps = ["p1", "p2", "p3"], tank = "t_head", flow = "fm1.flow" }
```

O `jk check` verifica que as instâncias existem, que a classe e a major são as do membro (com as capacidades de `@requires`), que o número cabe em `[min..max]` e que não há ciclos (`J0650`–`J0654`, ref/01 §5.13). A `p1` continua a ser uma instância com o seu nome; `station.pumps.p1.running` e `p1.running` são o mesmo sinal (documento 16, P6).

### 5.3 Virtuais: uma app implementa a classe

É a peça que falta ao **autor de abstrações**, e está detalhada no §7. Uma app declara `#[joker::implements(Pump)]` (a classe `pump@1`): as saídas da classe tornam-se **comandos virtuais** que ela lê, e as entradas da classe tornam-se sinais que ela publica. Para quem usa a instância, não há diferença entre uma bomba real, composta ou virtual.

---

## 6. Camada 3: usar classes no SDK

### 6.1 O caso simples

```rust
use joker::prelude::*;
use joker::classes::std::{Led, Button};

#[joker::app(name = "hello_world", version = "0.5.0")]
pub struct HelloWorld {
    #[device] led: Led,
    #[device] botao: Button,
}

#[joker::cycle(period = "10ms")]
fn control(io: &mut CycleIo<HelloWorld>, _st: &mut ()) -> Result<()> {
    if io.botao.clicked()? {
        io.led.toggle();
    }
    Ok(())
}
```

```toml
[apps.hello_world.bind]
led   = "painel_led"         # uma instância de led@1
botao = "painel_botao"       # uma instância de button@1
```

O `st.premido_antes` da parte 4 da trilha desapareceu: a deteção do flanco é a consulta `clicked` da classe, calculada pelo núcleo (trilha, parte 7).

### 6.2 Muitos equipamentos

```rust
use joker::classes::std::{MeterAc3, Pump};                     // meter.ac3@1 → MeterAc3
use crate::classes::local::Tank;                               // tank@1 é do projeto (classes/local/tank.jkc)

#[joker::app(name = "station", version = "1.0.0")]
pub struct Station {
    #[device] pumps: Many<Pump, 1, 8>,                         // entre 1 e 8; tamanho fixado no init
    #[device] tank: Tank,
    #[device(requires = ["pressure"])] booster: Pump,         // só aceita bombas com a capacidade `pressure` (A4)
    #[device] meter: MeterAc3,
    #[param(unit = "m", default = 2.5)] level_high: Param<f64>,
    #[param(unit = "m", default = 1.0)] level_low: Param<f64>,
}
```

`Many<C, MIN, MAX>` leva os limites como dois parâmetros constantes (a forma `Many<Pump, 1..=8>` não é Rust válido, ref/04 §4.8.3). Os nomes Rust das classes são o nome da classe em UpperCamelCase, sem pontos (`meter.ac3` → `MeterAc3`, `hvac.vav` → `HvacVav`), e podem importar-se com outro nome (`use … MeterAc3 as Meter`). Um exemplo realista (duty/standby com rotação) está no §9.1. As operações de um `Many`:

| Operação | Semântica |
|---|---|
| `iter()`, `iter_mut()` | ordem estável: a do manifesto, ou por nome; os métodos dos handles recebem `&self`, e `iter()` chega para propor |
| `get("p2")` → `Option`, `by_name("p2")` → `Result` (`J-E0503`), `index(1)` | acesso por nome da instância ou posição |
| `len()` | fixado no `init`; muda só com uma nova geração (A10) |
| `p.name()`, `p.index()` | identidade da instância (nome no manifesto, posição) |
| `healthy()` | iterador sobre as instâncias com `health() == Ok` |
| `read_all()?` | snapshot de todas; `Err` se alguma obrigatória estiver em falha |

### 6.3 Ler uma instância inteira

```rust
let p1 = io.pumps.by_name("p1")?;                      // get() devolve Option; by_name() devolve Result
let s: PumpState = p1.read()?;                         // struct gerada: running, fault, speed: Option<f64>, pressure_out: Option<f64>
match p1.health() {
    Health::Ok => {}
    Health::Degraded(m) => log::warn!("p1 degradada: {m:?}"),   // capacidades em falha
    Health::Failed(m) => return Err(Error::quality(m)),          // membros obrigatórios em falha
}
```

### 6.4 Ciclo e async

Os handles existem nas duas vistas, com a mesma divisão das portas (documento 04, §3.4):

| Vista | Pode |
|---|---|
| `CycleIo` | ler membros e consultas do latch; chamar métodos (propostas); `read()`, `health()` |
| `TaskIo` | tudo o que é async: `pump.run_at(60.0).await` devolve os `WriteOutcome` das escritas expandidas (`MethodOutcome`), depois do commit; **ações**: `pump.reset().await`; `history()` de um membro (em `raw()`) |

### 6.5 Classes locais: organizar uma app grande sem registo

Nem tudo merece uma classe partilhada. Para organizar as portas de uma app, declara-se uma classe **na própria app**:

```rust
#[derive(joker::Class)]
pub struct Zone {
    #[input(unit = "°C")]  temp: Input<f64>,
    #[input]               occupied: Input<bool>,
    #[output(unit = "°C")] setpoint: Output<f64>,
    #[output(unit = "%")]  damper: Output<f64>,
}

#[joker::app(name = "floor_ctl", version = "0.1.0")]
pub struct FloorCtl {
    #[device] zones: Many<Zone, 1, 64>,
}
```

E liga-se por modelo de caminho, sem repetir 64 vezes:

```toml
[apps.floor_ctl.bind.zones]
each = "bms.zone_*"                                  # zone_01 … zone_24
map  = { temp = "{}.temp", occupied = "{}.occ", setpoint = "{}.sp", damper = "{}.damper" }
```

Isto responde diretamente à questão da struct com 75 portas: **as portas agrupam-se pelo equipamento a que pertencem**, com classes do registo quando o conceito é partilhado e com classes locais quando é só organização.

### 6.6 TypeScript e Python

```ts
import { app, device, cycle } from "@joker/sdk";
import { Led, Button } from "@joker/classes-std";

export default app({
  name: "hello_world", version: "0.5.0",
  devices: { led: device(Led), botao: device(Button) },
  cycles: { control: cycle("100ms", (io) => { if (io.botao.clicked()) io.led.toggle(); }) },
});
```

```python
from joker_sdk import App, app, cycle, device
from joker_classes.std import Led, Button

@app(name="hello_world", version="0.5.0")
class HelloWorld(App):
    led = device(Led)
    botao = device(Button)

    @cycle("500ms")
    def control(self) -> None:
        if self.botao.clicked():
            self.led.toggle()
```

Os períodos são os mínimos de tier 2 (100 ms em TypeScript, 500 ms em Python, `J-E0854`; documento 04, §4.3). Com eles, um clique mais curto do que o período perde-se, porque `clicked` é avaliado no latch de quem o consome (A7). Para botões em TS ou Python, a forma certa é uma tarefa `task.on(…)` sobre o sinal do botão, ou uma implementação ativa em Rust (ref/05 §14.3).

---

### 6.7 Rust livre: onde vive o comportamento

Os métodos de uma classe partilhada são declarativos (§4.5) porque **qualquer um os pode chamar**: apps em Rust, TypeScript ou Python, a CLI (`jk write luz.mode fault`), a ponte OPC UA, uma intenção aprovada. Para todos executarem o mesmo `toggle`, o núcleo tem de o saber executar, e o `jk check` tem de saber que saídas ele escreve. Isso não impede Rust livre: só o põe no sítio certo. Há três, conforme **a quem pertence o comportamento**.

| Onde | Rust livre? | Corre | Estado | Quem o pode usar | Para quê |
|---|---|---|---|---|---|
| **extension trait** sobre o handle | sim | na app que chama | o da app que chama | só apps Rust que importem o crate | conveniências de quem usa: "como eu gosto de usar um LED" |
| **`impl` de uma classe local** (§6.5) | sim | na app | o da app | só essa app | organizar e dar comportamento a portas agrupadas |
| **implementação ativa** (§7) | sim | numa app própria, em ciclo | o do equipamento, um só | todos os chamadores, incluindo CLI, HMI e pontes | comportamento que pertence ao equipamento: sequências, proteções, padrões |

**Extension traits: comportamento do lado de quem chama.** Não exigem nada ao núcleo e distribuem-se como crates:

```rust
// crate joker-led-ext (de quem quiser)
pub trait LedExt {
    fn toggle_debounced(&mut self, st: &mut ToggleState, now: Instant);
}

impl LedExt for LedCycle<'_> {                 // o handle de ciclo; `Led` é só a declaração
    fn toggle_debounced(&mut self, st: &mut ToggleState, now: Instant) {
        if now - st.last >= 500.ms() {   // Rust livre
            self.toggle();               // no fim, usa os métodos de contrato
            st.last = now;
        }
    }
}
```

O código corre na instância Wasm da app, é determinista e o replay é exato. Tem dois limites, que são também a regra para escolher: só existe em Rust, e cada app que o usa tem a sua cópia do estado. Se o comportamento tem de ser **o mesmo para todos os chamadores**, ou tem estado que pertence ao equipamento (um piscar, uma proteção contra ciclos curtos), o sítio é a implementação ativa, como a `status_light_ctl` da trilha (parte 8).

**A regra, numa linha:** o *contrato* (membros, métodos, consultas) é declarativo e neutro; o *comportamento* é Rust, do lado de quem chama (extension trait) ou do lado do equipamento (implementação ativa).

## 7. Para o autor de abstrações: implementações ativas

As classes declarativas cobrem o mapeamento. O que não cobrem é **comportamento**: sequência de arranque com rampa, proteção contra ciclos curtos, alternância automática entre duas bombas, uma válvula que precisa de "abrir 100% durante 2 s e depois regular". É exatamente isto que um autor de abstrações quer escrever uma vez e reutilizar em cem sites.

### 7.1 Uma app que implementa uma classe

```rust
use joker::prelude::*;
use joker::classes::std::Pump;

/// Implementa pump@1 sobre um contactor e um retorno, com proteção contra ciclos curtos.
#[joker::app(name = "pump_ctl", version = "1.2.0")]
#[joker::implements(Pump, capabilities = ["hour_counter"])]   // gera `io.cmd` (comandos recebidos) e `io.state` (a publicar)
pub struct PumpCtl {
    #[output] contactor: Output<bool>,        // camada 1: o hardware real
    #[input]  feedback: Input<bool>,
    #[input]  thermal_trip: Input<bool>,
    #[param(unit = "s", default = 180.0)] min_off: Param<f64>,
    #[param(unit = "s", default = 60.0)]  min_on: Param<f64>,
}

struct Ctl {
    on: bool,
    since_s: f64,
}

impl Ctl {
    // Estado inicial: como se a bomba estivesse parada há `min_off`, para o primeiro arranque ser imediato.
    fn init(io: &CycleIo<PumpCtl>) -> Self { Ctl { on: false, since_s: io.min_off.get() } }
}

#[joker::cycle(period = "100ms", init = Ctl::init)]   // sem `init`, o estado começa em Default
fn run(io: &mut CycleIo<PumpCtl>, st: &mut Ctl) -> Result<()> {
    st.since_s += io.dt().as_secs_f64();
    let want = io.cmd.run_cmd.get()?;                   // o que a camada de cima pediu

    let can_start = !st.on && st.since_s >= io.min_off.get();
    let can_stop  =  st.on && st.since_s >= io.min_on.get();
    if (want && can_start) || (!want && can_stop) {
        st.on = want;
        st.since_s = 0.0;
    }
    io.contactor.set(st.on);

    // Publicar o estado da classe: quem usa a bomba vê isto.
    io.state.running.set(io.feedback.get()?);
    io.state.fault.set(io.thermal_trip.get()?);
    // Sem `variable_speed` na lista de capacidades, `io.state.speed` não existe (erro de compilação).
    // io.state.hours (hour_counter): contadas aqui e persistidas em kv por uma tarefa async (omitida)
    Ok(())
}
```

```toml
[instances.p1]
class = "pump@1"
implemented_by = "app:pump_ctl"        # instância virtual: uma instância da app, com o bind de [apps.pump_ctl]

[apps.pump_ctl]
path = "apps/pump_ctl"
bind = { contactor = "io.do_1", feedback = "io.di_1", thermal_trip = "io.di_2" }

[outputs."io.do_1"]
safe = false                           # envelope do hardware real

[outputs."p1.run_cmd"]                 # envelope do comando virtual: sim, também tem
safe = false
max_actuations = "6/h"
```

### 7.2 Semântica das camadas

- **Os comandos virtuais são saídas como as outras:** têm árbitro, envelope e `safe`. Duas apps de negócio a mandar na mesma bomba virtual resolvem-se pelo árbitro, **antes** de chegarem à implementação.
- **Segurança em cada camada:** se a app de negócio morre, o lease de `p1.run_cmd` expira e o comando vai para `safe` (parar), e a `pump_ctl` para a bomba *respeitando o `min_on`*. Se a `pump_ctl` morre, o lease do `io.do_1` expira e o contactor abre. Nenhuma camada depende da de cima para ser segura.
- **Latência:** uma camada acrescenta **um tick** do grupo da implementação (o comando é aplicado no fim do tick do consumidor e lido no latch seguinte). Com as duas no mesmo grupo e `order = ["consumer", "implementer"]` no grupo (documento 03, §7), o núcleo arbitra e aplica o envelope aos comandos virtuais logo a seguir à tarefa que os produz (documento 01, §4, passo 4b), e a latência fica nula. O `jk explain p1` mostra a latência de cada camada.
- **Profundidade:** no máximo 3 camadas virtuais em cadeia (`J0620`). Mais do que isso raramente é abstração: é indireção.

### 7.3 Testes de conformidade da classe

Uma classe pode trazer **cenários que qualquer implementação tem de passar**. É o que dá confiança a quem usa uma bomba de outra pessoa.

```toml
# classes/std/pump.conformance.toml
schema = 1

[suite]
kind  = "class"
class = "std/pump@1"

[[case]]
id     = "start_running_or_failed"
name   = "start → running em ≤ 10 s, ou failed_to_start"
do     = [{ at = "1s", method = "start" }]
expect = [{ any_of = [{ member = "running", equals = true, before = "11s" },
                      { query = "failed_to_start", equals = true, at = "11.1s" }] }]

[[case]]
id       = "stop_zeroes_speed"
name     = "stop leva speed_set a 0 %"
requires = ["variable_speed"]
do       = [{ at = "1s", method = "run_at", args = { pct = "60 %" } }, { at = "5s", method = "stop" }]
expect   = [{ member = "speed_set", equals = "0 %", at = "5.2s" }]

[[case]]
id       = "safe_stops_pump"
name     = "comando em safe → a bomba para"
sim_only = true           # posse exclusiva: numa instância real, `skipped (sim_only)` (ref/03 §2.6.3)
do       = [{ at = "1s", method = "start" }, { at = "20s", lease_expire = "run_cmd" }]
expect   = [{ member = "running", equals = false, before = "90s" }]     # a classe admite até 60 s de tempo mínimo de marcha
```

É o **mesmo esquema** dos casos de conformidade do núcleo (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), com os literais canónicos em minúsculas; só a *fixture* muda. O ficheiro tem o nome do `.jkc` (`pump.jkc` ↔ `pump.conformance.toml`), e cada caso tem um `id`. O esquema completo está na ref/03 §2.6.

```sh
$ jk class verify pump@1 --instance p1 --env sim               # corre os casos contra a instância, em simulação
$ jk class new valve@1                                         # esqueleto de classe + ficheiro de conformidade
$ jk class publish classes/local/station.jkc
```

Um perfil com `implements` e uma app com `#[implements]` só podem ser publicados no registo se passarem os casos da classe.

---

## 8. Estresse: 14 ataques ao desenho

**A1 — O LED simples.** `led.toggle()` em vez de `Output<bool>` e de um estado manual. Aguenta. O custo em runtime é mínimo: o método vai como um registo no buffer de propostas e o núcleo expande-o numa proposta no membro `on` (documento 17, §4.1).

**A2 — A bomba em três dispositivos.** As instâncias compostas (§5.2) resolvem o caso que os modelos "um dispositivo = uma classe" (como os *device profiles* de muitas plataformas IoT) não resolvem. Um membro de uma instância composta não tem coerência de aquisição com os outros, porque vêm de pedidos diferentes: o `read()` é coerente por tick (latch), e não por pedido (documento 02, §6.2). Fica escrito na documentação de `read()`.

**A3 — 120 VAVs num edifício.** Com instâncias implícitas (perfil BACnet com `implements."hvac.vav@1"`), os 120 dispositivos são 120 instâncias sem uma linha extra, e a app liga `vavs = "class:hvac.vav@1"` (todas) ou `"vav_3*"` (um piso). Na imagem de processo, 120 × 6 membros = 720 portas = ~17 KB por tick, dentro do orçamento (documento 09, ataque 2).

**A4 — Inversores de cinco fabricantes.** A classe `inverter.pv@2` tem um núcleo que todos cumprem (potência ativa, limite de potência, estado) e capacidades para o resto (`reactive`, `curtail_ramp`, `mppt_detail`). Uma app que precisa de uma capacidade declara-a (`#[device(requires = ["reactive"])]`), e o `jk check` falha (`J0602`) nos sites onde alguma instância não a tem, **antes** do deploy. O que é mesmo específico de um fabricante fica acessível por `raw()` ou por uma porta de camada 1 ao lado. O erro a evitar é a classe "que tem tudo": cada membro obrigatório a mais é um fabricante a menos que a cumpre.

**A5 — Um método que escreve em duas saídas.** `battery.set_power(p)` tem de escrever o modo e o setpoint de forma coerente. Com `atomic = true` e `order` na definição do método, a instância gera um grupo atómico (M20) sobre esses membros. Quem escreve o método não tem de se lembrar disso em cada site.

**A6 — `toggle()` com um override manual ativo.** Se o operador forçou o LED a `true` no nível `manual:4` e a app chama `toggle()`, deve a app inverter o valor **efetivo** (o do operador) ou o **seu**? Decisão: **o seu**. `own(on)` é a última proposta da app no seu nível do árbitro; sem proposta anterior (primeira chamada, ou lease expirado), é o valor aplicado (documento 16, §4). Se fosse o efetivo, cada toggle durante o override inverteria o valor do operador na camada da app, e quando o override expirasse o LED ficaria num estado que ninguém escolheu. Com `own`, a camada da app continua coerente com a sua própria lógica, e o override não a contamina.

**A7 — `clicked` precisa de estado (o valor anterior).** Onde vive esse estado? Hipóteses: (a) no SDK, dentro do handle, na memória da app; (b) no núcleo, como sinal derivado. Com (a), haveria três implementações de `rose()` (Rust, TS, Python) além da do núcleo, a divergir, que é o erro que o Cravo proíbe (partilhar semântica sem partilhar código). **Decisão: (b).** As consultas das classes são **sinais derivados avaliados pelo núcleo**, no latch de cada grupo que as consome, com o mesmo bytecode das expressões do manifesto. Um só avaliador, e o replay continua exato. O custo é que as consultas são limitadas à linguagem de expressões, e quem precisar de mais usa uma implementação ativa. Há ainda um limite deliberado: as consultas têm **estado volátil** (janelas ≤ 15 min, flancos, temporizadores) e recomeçam num reinício. O que tem de sobreviver a reinícios, como as horas de funcionamento, é um **membro** publicado por quem o sabe (o drive, ou uma implementação ativa que o guarda em `kv`), e não uma consulta.

**A8 — Qualidade de uma instância.** Com 7 membros de 3 dispositivos, "a bomba está boa?" não tem uma resposta evidente. Decisão: `health required(running, fault)` na classe (documento 17, §3.1). `health()` é `Ok` se os obrigatórios estiverem `Good` e as capacidades também, `Degraded` se só falharem capacidades, e `Failed` se falhar algum obrigatório. `read()?` falha em `Failed`. Os membros individuais continuam acessíveis com a sua qualidade. Não se inventa uma qualidade média.

**A9 — `pump.reset()` numa tarefa de ciclo.** É uma ação, logo só existe em `TaskIo` (documento 02, §8). O handle de ciclo não tem o método, e é um erro de compilação, e não de runtime.

**A10 — Um site ganha uma quarta bomba.** O tamanho de um `Many<Pump, 1, 8>` é fixado no `init`. Uma geração que muda o conjunto de instâncias reinicia a app com `migrate` (documento 01, §9), e o estado por instância (`PerInstance`, ref/04 §4.8.3) é guardado **por nome de instância**, de forma que `p1`–`p3` mantêm o seu estado e `p4` começa do `Default`. Fora dos limites declarados, `J0610` no `jk check`.

**A11 — Versões.** Uma app que depende de `pump@1` aceita instâncias de `pump@1.x`, porque o minor só acrescenta membros opcionais e capacidades. Uma instância de `pump@2` não liga (`J0630`). As mudanças major de classes padrão acompanham a política M21, com a versão anterior suportada durante uma major da runtime.

**A12 — Desempenho.** As classes são resolvidas no build e no `jk check`: cada membro é uma porta, cada método é um registo de ~16 B expandido pelo núcleo, cada consulta é um sinal derivado. Não há dispatch dinâmico, reflexão ou lookup por nome no tick. Uma app com 15 equipamentos × 5 membros tem 75 portas, 1,8 KB de imagem, e o mesmo custo de uma app escrita com portas cruas.

**A13 — Testar sem hardware.** Cada classe padrão tem um **simulador de classe** (`classes/<pacote>/<nome>.sim.toml`, ref/03 §2.4.11): `[instances.p1] class = "pump@1", sim = true` cria uma bomba simulada com dinâmica plausível (arranque com atraso, falhas injetáveis, pressão proporcional à velocidade) e presença no painel do `jk dev`. Os testes de conformidade da classe (§7.3) correm contra ele.

**A14 — Classes a mais.** O risco oposto: uma classe para cada coisa, 400 classes num registo que ninguém domina. Contenção: a biblioteca `std/` é pequena e revista (§10); as classes de projeto são livres; as classes locais de app (§6.5) não entram no registo. E a regra de ouro: **se duas apps de sites diferentes não precisam de falar do mesmo conceito, não é uma classe partilhada.**

---

## 9. Três casos grandes

### 9.1 Estação de bombagem: 15 equipamentos

3 bombas (compostas: contactor + retorno + térmico + pressão), 2 tanques (nível 4–20 mA + boias de alarme), 1 medidor de caudal (Modbus), 1 medidor de energia (Modbus), 4 válvulas motorizadas (abrir/fechar/fins de curso), 3 alarmes de intrusão e inundação (DI), 1 gerador (Modbus).

```toml
# Três bombas, a mesma implementação: uma instância da app por bomba, cada uma com as suas ligações.
[instances.p1]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_1", feedback = "io.di_1", thermal_trip = "io.di_2" } }
[instances.p2]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_2", feedback = "io.di_3", thermal_trip = "io.di_4" } }
[instances.p3]
class = "pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_3", feedback = "io.di_5", thermal_trip = "io.di_6" } }

[instances.t_suction]
class = "local/tank@1"                                # classe do projeto: a std/ não tem tanques
map = { level = "ai.ch2", high_float = "io.di_7", low_float = "io.di_8" }
[instances.t_head]
class = "local/tank@1"
map = { level = "ai.ch3", high_float = "io.di_9", low_float = "io.di_10" }

# medidores, válvulas e gerador: instâncias implícitas pelos perfis

[apps.station.bind]
pumps  = ["p1", "p2", "p3"]
suction = "t_suction"
head    = "t_head"
valves  = "class:valve.motor@1"
flow    = "fm1"
energy  = "em1"
genset  = "gen1"
alarms  = ["io.di_11", "io.di_12", "io.di_13"]      # porta de camada 1, sem classe: não precisa de uma
```

```rust
use joker::prelude::*;
use joker::classes::std::{Genset, MeterAc3, Pump, ValveMotor};
use crate::classes::local::{FlowMeter, Tank};        // classes do projeto: não estão na std/ (§10)

#[joker::app(name = "station", version = "1.0.0")]
pub struct Station {
    #[device(requires = ["hour_counter"])] pumps: Many<Pump, 1, 8>,
    #[device] suction: Tank,
    #[device] head: Tank,
    #[device] valves: Many<ValveMotor, 0, 16>,
    #[device] flow: FlowMeter,
    #[device] energy: MeterAc3,
    #[device] genset: Genset,
    #[input(many)] alarms: InputMany<bool>,
    #[param(unit = "m", default = 2.5)] start_level: Param<f64>,
    #[param(unit = "m", default = 3.6)] stop_level: Param<f64>,
}

#[derive(Default)]
struct Duty {
    demand: bool,
    lead: usize,          // bomba principal, escolhida no início de cada pedido
    failed: [bool; 8],    // failed_to_start no tick anterior: conta-se só o flanco
}

#[joker::cycle(period = "500ms")]
fn control(io: &mut CycleIo<Station>, st: &mut Duty) -> Result<()> {
    // Segurança de processo primeiro: sem água na aspiração, nada arranca.
    if io.suction.low_float()? || io.alarms.iter().any(|a| a.get().unwrap_or(true)) {
        io.pumps.iter().for_each(|p| p.stop());
        st.demand = false;
        return Ok(());
    }

    let level = io.head.level()?;
    if !st.demand && level < io.start_level.get() {
        // Início de um pedido: a principal é a saudável com menos horas. Escolhida só aqui,
        // e não em todos os ticks, duas bombas com horas próximas não trocam a meio do pedido.
        st.demand = true;
        let lead = io.pumps.healthy().min_by(|a, b| {
            a.hours().unwrap_or(f64::INFINITY).total_cmp(&b.hours().unwrap_or(f64::INFINITY))
        });
        if let Some(p) = lead {
            st.lead = p.index();
        }
    } else if st.demand && level > io.stop_level.get() {
        st.demand = false;
    }

    let n = io.pumps.len();                        // fora do ciclo: iter() empresta io.pumps
    for p in io.pumps.iter() {                     // os métodos são &self: não é preciso iter_mut()
        let i = p.index();
        if st.demand && i == st.lead { p.start() } else { p.stop() }
        let failed = p.failed_to_start()?;
        if failed && !st.failed[i] {
            st.lead = (st.lead + 1) % n;           // a principal falhou o arranque: passa à seguinte
        }
        st.failed[i] = failed;
    }
    Ok(())
}
```

A primeira versão deste exemplo reescolhia a principal em todos os ticks pela menor contagem de horas: com horas próximas, as bombas trocavam de principal com a bomba a trabalhar, e o parâmetro `rotate_every` não era usado. Agora a rotação é por pedido, que é o que iguala as horas sem arranques a mais (ref/05 §20.2 tem a mesma lógica em TypeScript e Python). Também usava `enumerate()` como índice, que só coincide com `p.index()` por acaso, e `io.pumps.len()` dentro de um `iter_mut()`, que o borrow checker recusa.

Umas quarenta linhas de lógica para 15 equipamentos, sem um único registo, pino ou escala. Os perfis, as instâncias compostas e a `pump_ctl` (proteção contra ciclos curtos) vivem noutras camadas, feitas e testadas por quem sabe de hardware.

### 9.2 Edifício: 120 VAVs e 4 UTAs

```rust
use joker::prelude::*;
use joker::classes::std::{HvacAhu, HvacVav};          // hvac.ahu@1, hvac.vav@1

#[joker::app(name = "hvac_opt", version = "2.0.0")]
pub struct HvacOpt {
    #[device] ahus: Many<HvacAhu, 1, 8>,
    #[device] vavs: Many<HvacVav, 1, 512>,
}

#[joker::task(every = "5m")]
async fn optimize(io: &TaskIo<HvacOpt>) -> Result<()> {
    for ahu in io.ahus.iter() {
        let worst = io.vavs.iter()
            .filter(|v| v.served_by().is_ok_and(|s| s == ahu.name()))   // served_by() é um Result
            .filter_map(|v| v.temp_error().ok())
            .fold(0.0_f64, f64::max);
        let sp = if worst > 1.0 { ahu.supply_sp()? - 0.5 } else { ahu.supply_sp()? + 0.2 };
        ahu.set_supply_sp(sp).await?;
    }
    Ok(())
}
```

`served_by` é um **param** da classe `hvac.vav@1`, dado pelo perfil ou pela instância. É assim que as relações entre equipamentos (esta VAV é servida por aquela UTA) entram sem um modelo de grafo à parte. Se um dia for preciso mais, a relação é um membro, e não uma estrutura nova.

### 9.3 Bomba de calor: três níveis, uma implementação e um Joker Micro

Uma bomba de calor ar-água numa moradia: um compressor de velocidade variável num drive Modbus, duas sondas (ida e retorno), um contador trifásico e a entrada SG-Ready (dois contactos). Para a app de negócio é uma coisa só: produz calor com uma temperatura de ida, e aceita sinais da rede. É o controlador U2 do documento 19.

**As classes.** Três níveis: `heat_pump` → `compressor` → `std/drive@1`.

```
// classes/local/compressor.jkc
package local

/// Compressor de velocidade variável; o acionamento é um std/drive@1.
class compressor@1.0.0 {
    member drive: std/drive@1

    in discharge_temp: f64 °C

    method start(pct: f64 %) { drive.run_at(pct) }       // chama o método do membro (17 §4.1)
    method stop()            { drive.stop() }

    query overheated: bool = held(discharge_temp > 110 °C, 10 s)

    health required(discharge_temp)
}
```

```
// classes/local/sg_ready.jkc
package local

/// Entrada SG-Ready: dois contactos, quatro estados (1:0 bloqueio, 0:0 normal, 0:1 reforço, 1:1 forçado).
class sg_ready@1.0.0 {
    out c1: bool                  @safe_hint(false)
    out c2: bool                  @safe_hint(false)

    atomic method block()  order(c1, c2) { c1 = true;  c2 = false }
    atomic method normal() order(c1, c2) { c1 = false; c2 = false }
    atomic method boost()  order(c1, c2) { c1 = false; c2 = true }
}
```

```
// classes/local/heat_pump.jkc
package local

enum HpMode { off = 0, heat = 1, boost = 2 }

/// Bomba de calor: produz calor para o edifício, com uma temperatura de ida pedida.
class heat_pump@1.0.0 {
    member compressor: local/compressor@1
    member t_supply:   std/temp_sensor@1
    member t_return:   std/temp_sensor@1
    member meter:      std/meter.ac3@1
    member grid:       local/sg_ready@1       @optional

    out mode:    HpMode                        @safe_hint(HpMode.off)
    out flow_sp: f64 °C                        @safe_hint(35 °C) @range_hint(20 °C, 55 °C)

    method off() { mode = HpMode.off }
    atomic method heat(sp: f64 °C) order(flow_sp, mode) {
        flow_sp = clamp(sp, 20 °C, 55 °C)
        mode    = HpMode.heat
    }

    query delta_t:    f64 K = t_supply.temperature - t_return.temperature
    query overheated: bool  = compressor.overheated
}
```

- A `std/drive@1` (§10) ainda não tem a definição escrita. O exemplo usa dela `run_at(pct)`, `stop()` e `speed`, com a forma da `pump@1`.
- A `heat_pump@1` não tem `health`: a saúde vem dos membros (documento 17, §4.5). Com a sonda de ida em falta, a `t_supply` está `Failed`, e a bomba também. Com o SG-Ready em falta, a bomba fica `Degraded`, porque `grid` é `@optional`.
- **Os métodos da `heat_pump` só escrevem nos seus `out`.** Um `off()` que chamasse `compressor.stop()` seria natural numa instância composta, mas esta é virtual: quem manda no compressor é a implementação, e a chamada faria da app de negócio um segundo escritor do drive. O `jk check` recusa-o (`J0656`, documento 17, §4.1). O `compressor`, que é composto, chama os métodos do `drive` sem problema.

**No Pi, com a implementação em Wasm.**

```toml
# joker.toml (excerto): os envelopes do vfd1 e das saídas io.do_1 e io.do_2 estão omitidos
[devices.vfd1]                    # instância implícita de std/drive@1, pelo perfil
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 1
profile = "acme/hp-drive@2"

[devices.em1]                     # instância implícita de std/meter.ac3@1
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 2
profile = "acme/em3@1"

[instances.comp1]
class = "local/compressor@1"
map   = { drive = "vfd1", discharge_temp = "ai.ch3" }

[instances.t_ida]
class = "std/temp_sensor@1"
map   = { temperature = "ai.ch1" }

[instances.t_retorno]
class = "std/temp_sensor@1"
map   = { temperature = "ai.ch2" }

[instances.sg]
class = "local/sg_ready@1"
map   = { c1 = "io.do_1", c2 = "io.do_2" }

[instances.hp]
class          = "local/heat_pump@1"
implemented_by = "app:hp_ctl"
map            = { compressor = "comp1", t_supply = "t_ida", t_return = "t_retorno", meter = "em1", grid = "sg" }   # só membros de classe

[outputs."hp.mode"]
safe = "off"

[outputs."hp.flow_sp"]
safe = "35 °C"

[apps.hp_ctl]
path = "apps/hp_ctl"
bind = { compressor = "comp1", supply = "t_ida", ret = "t_retorno", grid = "sg" }

[apps.conforto]
path = "apps/conforto"
bind = { hp = "hp", exterior = "meteo.t_ext" }
```

Numa instância virtual, o `map` só liga os membros de classe; os escalares (`mode`, `flow_sp`) são comandos virtuais, e os `in`, se os houvesse, seriam publicados pela implementação (ref/01 §5.13).

```rust
// apps/hp_ctl/src/lib.rs: a implementação (a curva é só para o exemplo)
#[joker::app(name = "hp_ctl", version = "1.0.0")]
#[joker::implements(HeatPump)]
pub struct HpCtl {
    #[device] compressor: Compressor,
    #[device] supply: TempSensor,
    #[device] ret: TempSensor,
    #[device] grid: Option<SgReady>,
}

#[joker::cycle(period = "1s")]
fn run(io: &mut CycleIo<HpCtl>, _st: &mut ()) -> Result<()> {
    let mode = io.cmd.mode.get()?;
    match mode {
        HpMode::Off => io.compressor.stop(),
        _ => {
            let erro_k = io.cmd.flow_sp.get()? - io.supply.temperature()?;
            io.compressor.start((30.0 + 10.0 * erro_k).clamp(0.0, 100.0));   // em %
        }
    }
    if let Some(g) = &io.grid {
        if mode == HpMode::Boost { g.boost() } else { g.normal() }
    }
    Ok(())
}
```

```rust
// apps/conforto/src/lib.rs: a app de negócio
#[joker::app(name = "conforto", version = "1.0.0")]
pub struct Conforto {
    #[device] hp: HeatPump,
    #[input(unit = "°C")] exterior: Input<f64>,
}

#[joker::cycle(period = "10s")]
fn run(io: &mut CycleIo<Conforto>, _st: &mut ()) -> Result<()> {
    io.hp.heat((45.0 - 0.8 * io.exterior.get()?).clamp(25.0, 55.0));   // curva de aquecimento
    if io.hp.compressor.overheated()? {                                  // leitura pelo membro aninhado
        log::warn!("compressor quente: ida {:.1} °C", io.hp.t_supply.temperature()?);
    }
    Ok(())
}
```

A `conforto` lê o compressor e as sondas pelos handles aninhados (ref/04 §4.8.6), mas não escreve neles: são da `hp_ctl`. Os caminhos `hp.compressor.drive.speed`, `comp1.drive.speed` e `vfd1.speed` são o mesmo sinal (documento 16, P6).

```sh
$ jk explain hp
```

```
hp · local/heat_pump@1 · instância virtual, implementada por app:hp_ctl (joker.toml:31)
  mode          out HpMode    escritores: conforto (control:12) · safe = off
  flow_sp       out f64 °C    escritores: conforto (control:12) · safe = 35 °C
  delta_t       consulta      t_supply.temperature - t_return.temperature
  overheated    consulta      compressor.overheated
  compressor    comp1 · local/compressor@1 · composta · Ok
    drive       vfd1 · std/drive@1 · implícita (acme/hp-drive@2) · Ok
  t_supply      t_ida · std/temp_sensor@1 · composta · Ok
  t_return      t_retorno · std/temp_sensor@1 · composta · Ok
  meter         em1 · std/meter.ac3@1 · implícita (acme/em3@1) · Ok
  grid          sg · local/sg_ready@1 · composta · Ok · opcional
  saúde         Ok
  profundidade  3: heat_pump → compressor → drive (JC0501: máximo 3)
  camadas       conforto → hp → hp_ctl: 1 camada virtual (J0620: máximo 3)
```

**Com a `hp_ctl` num Joker Micro.** No controlador U2 (ESP32-C6), a `hp_ctl` é uma app nativa do MCU, e as instâncias `comp1`, `t_ida`, `t_retorno`, `sg`, `em1`, `vfd1` e `hp` estão no manifesto do MCU (`micro/hp/joker.toml`), com os mesmos blocos acima. O Pi vê o MCU como um dispositivo:

```toml
# joker.toml do Pi (excerto)
[devices.bomba]
driver    = "joker-link"
bus       = "rs485"
micro     = "micro/hp/joker.toml"
instances = { hp = "bomba_calor" }          # só o nome de topo se renomeia
```

O catálogo do `HELLO` leva a instância composta com a árvore de membros (ref/07 §6.3). O Pi vê **uma** instância, `bomba_calor`, da `local/heat_pump@1`, com os mesmos caminhos aninhados: `bomba_calor.compressor.drive.speed`, `bomba_calor.t_supply.temperature`. As instâncias que no MCU são só membros (`comp1`, `t_ida`…) não ganham nome no Pi: existem pelos caminhos da `bomba_calor`. A `conforto` corre no Pi sem mudar uma linha, com `bind = { hp = "bomba_calor" }`; as suas escritas em `bomba_calor.mode` são propostas pelo link, e o `safe` do `mode` aplica-se no MCU (documento 19, §6).

**Onde bate no limite.** Um condomínio com quatro bombas pede uma classe do site:

```
// classes/local/site.jkc: não compila
package local

class site@1.0.0 {
    member hps: local/heat_pump@1[1..4]
    query any_overheated: bool = any(hps.overheated)
}
```

```
erro[JC0501]: composição com profundidade 4 (máximo 3)
 --> classes/local/site.jkc:4:5
  |
4 |     member hps: local/heat_pump@1[1..4]
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ site → heat_pump → compressor → drive
  = ajuda: agrega o site numa app (`Many<HeatPump, 1, 4>`) ou numa implementação ativa
```

O site não é um equipamento: é uma política sobre quatro. Vai para uma app:

```rust
#[joker::app(name = "site_ctl", version = "1.0.0")]
pub struct SiteCtl {
    #[device] hps: Many<HeatPump, 1, 4>,
    #[input(unit = "kW")] limite: Input<f64>,
}

#[joker::cycle(period = "5s")]
fn run(io: &mut CycleIo<SiteCtl>, _st: &mut ()) -> Result<()> {
    let limite = io.limite.get()?;
    let mut total = 0.0;
    for h in io.hps.iter() {                          // ordem do bind: as últimas cortam-se primeiro
        total += h.meter.p_total().unwrap_or(0.0);    // p_total: potência ativa da meter.ac3@1
        if total > limite { h.off() }                 // sem proposta, manda a conforto
    }
    Ok(())
}
```

A `site_ctl` e a `conforto` escrevem as duas no `mode` de cada bomba. Com a `site_ctl` em `writer = { level = "supervisor" }`, o corte ganha à curva de aquecimento enquanto dura, e quando a `site_ctl` deixa de propor, o lease expira e volta a mandar a `conforto` (documento 16, A5). No mesmo nível, seria `J0210`.

---

## 10. Biblioteca padrão inicial (`classes/std/`) e normas

**Esta tabela é a lista normativa da `std/` da 1.0.** Os documentos 11, 13 e 21 remetem para aqui; uma classe fora dela entra numa minor, com ADR.

| Classe | Membros principais | Alinhada com (a validar) |
|---|---|---|
| `led@1`, `lamp@1`, `relay@1` | `on` (+ `brightness`) | — |
| `button@1`, `switch@1` | `pressed` / `closed` | — |
| `analog_in@1`, `analog_out@1` | `value` (0..1) + `reading` (o valor físico, capacidade `physical`; `raw` é reservado, `JC0207`) | — |
| `temp_sensor@1`, `humidity_sensor@1`, `pressure_sensor@1` | valor com unidade | — |
| `pump@1`, `fan@1` | `run_cmd`, `speed_set`, `running`, `speed`, `fault` | CiA 402 quando é um drive |
| `valve.motor@1`, `valve.modulating@1` | `open_cmd`/`position_set`, fins de curso | — |
| `meter.ac3@1` | P, Q, S, U/I por fase, f, energia | SunSpec 201–204, IEC 61850 MMXU |
| `inverter.pv@2` | P, limite de P, estado; capacidades `reactive`, … | SunSpec 103 e série 700 |
| `battery.bess@1` | SOC, SOH, P set/atual, modo, limites | SunSpec série 800, IEC 61850 ZBAT |
| `evse@1` | estado, corrente máxima, sessão | OCPP (visto do lado do carregador) |
| `hvac.ahu@1`, `hvac.vav@1` | temperaturas, setpoints, caudais, estado | objetos BACnet, Brick/Haystack |
| `drive@1` | modo, velocidade, binário, palavra de estado | CiA 402 / PROFIdrive |
| `genset@1` | estado, P, combustível, arranque/paragem | — |

A regra: **onde há uma norma com modelo de dados, a classe segue-a**, e o perfil SunSpec implementa `inverter.pv@2` quase de graça. Uma classe padrão nova exige ADR. Tanques, medidores de caudal e o resto do que é de um setor ficam como classes do projeto (`local/`) ou de pacotes (`acme/water`), até duas apps de sites diferentes precisarem de falar do mesmo conceito (A14). Os handles gerados publicam-se como `joker-classes-std` (Rust, reexportado em `joker::classes::std`), `@joker/classes-std` (TypeScript) e `joker_classes.std` (Python).

---

## 11. O que não muda (e porque isto não parte o desenho)

| Invariante | Com classes |
|---|---|
| a app nunca escreve no hardware (ADR 0005) | métodos = propostas; instâncias virtuais = saídas com envelope |
| estado seguro obrigatório (ADR 0006) | `safe_hint` é sugestão; `safe` continua no manifesto, também nos comandos virtuais |
| a qualidade nunca desaparece | `health()` e `read()?` sobre os membros; nada de médias |
| um só avaliador de expressões | as consultas são sinais derivados do núcleo (A7) |
| determinismo e replay | tudo compila para portas e sinais que o traço já grava |
| custo no tick | zero além do das portas equivalentes (A12) |

---

## 12. Decisões rejeitadas

| Proposta | Porque ficou de fora |
|---|---|
| herança entre classes | hierarquias impossíveis de manter; composição + capacidades chegam |
| métodos com código livre na definição da classe | têm de ser chamáveis por todos (outras linguagens, CLI, pontes); o comportamento livre vai para extension traits ou implementações ativas (§6.7); métodos Wasm puros ficam em aberto (§13) |
| consultas avaliadas no SDK | um segundo avaliador que diverge do núcleo (A7) |
| classes a definir `safe`, gamas e interlocks | a segurança é do site, não do tipo de equipamento (ADR 0006) |
| descoberta dinâmica de classes em runtime ("que coisas existem?") | as ligações são resolvidas no `jk check`; uma app não descobre equipamentos novos às escondidas |
| `toggle` sobre o valor efetivo | contaminaria a camada da app com overrides alheios (A6) |
| classes obrigatórias | a camada 1 tem de continuar completa: há casos (diagnóstico, protocolos exóticos, protótipos) em que o sinal cru é o certo |

---

## 13. Decisões e alternativas futuras

**Formato da definição de classes: decidido.** As classes escrevem-se em `.jkc` (documento 17, ADR 0018). As classes locais em crates Rust continuam válidas dentro das apps; a extração automática de uma crate para `.jkc` (`jk class extract`) fica para depois da v1.

**Métodos como funções Wasm puras (alternativa futura).** Um método de contrato poderia ser uma função Rust pura compilada para Wasm, com as escritas declaradas na assinatura, e executada pelo host sempre que alguém a chamasse:

```rust
#[joker::method(writes = [on])]
fn toggle(cx: &MethodCx<Led>) -> Writes<Led> {
    Writes::new().on(!cx.own().on)       // Rust livre, sem estado e sem I/O
}
```

Daria Rust nos métodos partilhados, chamáveis por todos, e o `jk check` continuaria a saber o que é escrito. Custa uma peça nova no host, ~1–2 µs por chamada, e só serve para funções puras: o estado continua a precisar de uma implementação ativa. Fica registado, e só avança se aparecer um caso real que as extension traits e as implementações ativas não resolvam.
