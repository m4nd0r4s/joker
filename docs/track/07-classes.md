# Parte 7 — Classes: o mesmo sistema na camada de equipamentos

**Módulo B · Classes.** No Módulo A escreveste tudo na camada de **sinais**: `Output<bool>`, `Input<f64>`, flancos detetados à mão, e a tradução de "sinal" para "significado" dentro da app. Agora sobe-se uma camada, e o comportamento não muda.

**Objetivo:** reescrever a `luz` sobre **classes de equipamento** (`Led`, `Button`, `TempSensor`, `AnalogIn`), ver as três maneiras de nascer de uma instância na bancada, e provar com o cenário `falha_esp` da parte 5, sem lhe tocar, que o sistema faz exatamente o mesmo.

**Vais aprender:** classe, instância e membro · `jk class list` e `jk class show` · instâncias implícitas (`[implements]` num perfil) e o que a v1.0 não deixa fazer · `jk profile diff` · instâncias compostas no manifesto · `#[device]` e handles de classe · métodos e consultas calculadas pelo núcleo · `raw()` · `jk status --instances` e `jk explain <instância>` · `jk class verify` no hardware.

**Antes de começar:** a parte 6 feita: `joker.rpi.toml`, `joker.lab.toml`, `joker.zero.toml`, a `luz` **0.4.1** a correr no zero, o lab na geração 3 (sem o `esp`), e o cenário `sim/scenarios/falha_esp.scenario.toml` a passar. O ESP32 ligado por USB ao Pi 3 (partes 4–5) e visível do Pi Zero por Wi-Fi (parte 6).

---

**Primeiro, repor o lab.** Um mestre de cada vez (parte 6, §7): para o zero, e volta a pôr o lab no `joker.lab.toml` completo, com a `luz` 0.4.1 e o ESP32 por RTU (geração 7). O resto da parte começa no simulador.

```sh
zero$ sudo systemctl stop jokerd
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
```

## 1. Porquê, se já funciona?

Relê a tarefa `pisca` da 0.4.1 e separa as linhas em dois montes. Num, **o que o sistema faz**: ligado ou desligado, o meio período em função da temperatura, o toque a mexer no `t_quente`, o que fazer quando o ESP32 desaparece. No outro, **como se fala com as coisas**: guardar o valor anterior do botão para ver o flanco, saber que `esp.touch` a 60 é "sem dedo", saber que `pi.act_led` é um `bool`. O segundo monte repete-se em todas as apps com um botão ou um LED. As classes tiram-no da tua app.

```
 camada 3   a tua app: "um clique liga, quanto mais quente mais depressa"
 camada 2   classes: std/led@1, std/button@1, std/temp_sensor@1, std/analog_in@1   ← esta parte
 camada 1   sinais: pi.act_led, pi.cpu_temp, esp.led, esp.button, esp.touch      ← Módulo A
```

As classes são uma camada **por cima** dos sinais, e não em vez deles (documento 14, §1 e §3). Compilam para portas: o envelope, o árbitro, a qualidade, o historian e o replay continuam iguais, e a camada 1 continua lá para quem precisar.

## 2. Conhecer as classes

```sh
$ jk class list --std
```

```
std/led@1              1.0.0   Indicador luminoso
std/button@1           1.0.0   Botão de pressão momentâneo
std/analog_in@1        1.0.0   Entrada analógica normalizada (0..1)
std/temp_sensor@1      1.0.0   Sensor de temperatura
std/humidity_sensor@1  1.0.0   Sensor de humidade
std/relay@1            1.0.0   Relé
std/pump@1             1.2.0   Bomba com arranque/paragem e retorno de estado
std/meter.ac3@1, std/inverter.pv@2, std/battery.bess@1, …
```

```sh
$ jk class show std/button@1
```

```
std/button@1 (1.0.0) — Botão de pressão momentâneo
  membros     pressed     in     bool
  consultas   clicked     bool   rose(pressed)        verdadeiro durante um único tick do grupo que a lê
              released    bool   fell(pressed)
              long_hold   bool   held(pressed, 1 s)
```

Repara na consulta `clicked`. É o `premido && !st.premido` da tua 0.4.1, com o `premido` guardado no estado da tarefa. Aqui quem guarda o valor anterior é o **núcleo**: as consultas são sinais derivados, avaliados no latch de cada grupo que as lê, com o mesmo avaliador das expressões do manifesto (documento 14, A7). A tua app deixa de ter `premido`, e o Rust, o TypeScript e a CLI veem o mesmo flanco.

O `jk class show std/led@1` mostra o outro lado: um membro `out on: bool @safe_hint(false)`, os métodos `on()`, `off()`, `set(v)` e `toggle()`, e a capacidade opcional `dimmable`. Quando um método tem o nome de um membro, o handle dá o método e o membro fica em `raw().on` (documento 17, §3.2). Duas coisas para a secção 6: um **método** não é código, é uma atribuição que o núcleo expande numa proposta (documento 17, §4.1); e o `@safe_hint` é só uma sugestão. O `safe` do `pi.act_led` e do `esp.led` continua a ser o do teu `joker.toml` (ADR 0006).

## 3. Três maneiras de nascer

Uma **instância** diz "este equipamento é, para os programas, um `std/led@1`". Na bancada vais usar as três maneiras (documento 14, §5):

| Maneira | Quem a declara | Quando | Na bancada |
|---|---|---|---|
| **implícita** | o perfil do equipamento, com `[implements]` | o dispositivo **é** o equipamento | `esp`, pelo perfil `local/bench-esp32@1` |
| **composta** | o manifesto, com `[instances.<i>] map` | o equipamento é feito de sinais soltos, o dispositivo não tem perfil, ou o site quer um nome e uma calibração seus | `led_pi` e `cpu` (o driver `sysfs` não tem perfil); `led_esp`, `botao` e `toque` |
| **virtual** | uma app, com `#[joker::implements]` | a classe precisa de comportamento | `sinal`, na parte 8 |

## 4. Implícita: o perfil diz o que o ESP32 é

Quem sabe o que o firmware de bancada é, é o perfil que escreveste na parte 4: um LED numa coil, um botão num discrete input, uma leitura analógica num input register. Acrescenta-lhe as classes que ele cumpre:

```toml
# profiles/local/bench-esp32@1.profile.toml (acrescentar no fim)
[implements."std/led@1"]
map = { on = "led" }

[implements."std/button@1"]
map = { pressed = "button" }

# value = 1 − cru/100: a escala nominal do firmware (0 a 100), e não a calibração desta bancada
[implements."std/analog_in@1"]
map = { value = { signal = "touch", scale = -0.01, offset = 1 } }
```

**Vários `[implements]`, uma só instância.** A chave de cada tabela é a classe, e um perfil pode cumprir até 16 (ref/03 §2.1.5). Mas cada dispositivo com o perfil é **uma** instância implícita, com o nome do dispositivo (ref/03 §2.2.14): o teu `esp` passa a ser, ao mesmo tempo, um `std/led@1`, um `std/button@1` e um `std/analog_in@1`. Os membros ganham caminhos na instância (`esp.on` é um alias de `esp.led`, `esp.pressed` de `esp.button`), e é por isso que três classes cabem na mesma instância: não há dois membros com o mesmo nome, nem um membro com o nome de outro sinal do dispositivo (documento 16, P3; senão, `J0604`). A direção é verificada contra o perfil (um `out` só vai para uma `output`: `J0824`), e o `scale`/`offset` do `value` é o adaptador de 17 §5.2.

### O que o `jk check` diz se quiseres outro nome

A tentação é dar ao botão um nome próprio, `botao`, logo no perfil:

```toml
[implements."std/button@1"]
map       = { pressed = "button" }
instances = { botao = { pressed = "button" } }
```

```
erro[J0807]: a chave `instances` está reservada para uma versão futura (1.x)
 --> profiles/local/bench-esp32@1.profile.toml:24:1
   |
24 | instances = { botao = { pressed = "button" } }
   | ^^^^^^^^^ várias instâncias implícitas por dispositivo chegam na 1.x
   = nota: na 1.0, cada dispositivo com este perfil é uma instância implícita com o nome do dispositivo (`esp`)
   = ajuda: um nome diferente é uma instância composta no manifesto (ref/03 §2.2.14)
```

Apaga a linha. O perfil diz **o que o equipamento é**, para qualquer site que o use; os nomes do teu site vêm na secção 5.

### A versão do perfil sobe

Um `implements` novo é uma mudança **minor** (ref/03 §2.2.17). Pergunta à `jk`, contra a versão que está no Git:

```sh
$ git show HEAD:profiles/local/bench-esp32@1.profile.toml > /tmp/bench-esp32-antes.profile.toml
$ jk profile diff /tmp/bench-esp32-antes.profile.toml profiles/local/bench-esp32@1.profile.toml
```

```
local/bench-esp32   1.0.0 → 1.0.0
  + implements std/led@1          minor
  + implements std/button@1       minor
  + implements std/analog_in@1    minor
exige: minor (≥ 1.1.0); o ficheiro continua em 1.0.0
```

Muda `version = "1.1.0"` em `[profile]` e repete: `1.0.0 → 1.1.0 ✓ minor`. Um perfil `local/` nunca se publica (`J0848`), mas a versão importa na mesma: o `joker.lab.lock` e o `joker.zero.lock` fixam o perfil pela versão e pelo digest dos bytes do ficheiro (ref/02 §2.1.5), e o `jk check` em `dev` atualiza-os com uma nota.

**No simulador, o mesmo.** O driver `sim` não tem perfil (ref/01, anexo A): o modelo declara os seus sinais, e também as suas classes. Acrescenta ao `sim/esp.sim.toml` as mesmas três tabelas (ref/03 §2.4.9), para o `esp` ser a mesma instância nos dois mundos.

## 5. Compostas: os nomes e as calibrações do site

O dispositivo `pi` usa o driver `sysfs`, que lê os sinais diretamente do manifesto (`led = "ACT"`, `thermal = "thermal_zone0"`), sem perfil. Sem perfil não há `[implements]`, e as instâncias do Pi escrevem-se no manifesto. As do ESP32 também, por três razões que a secção 4 já deixou ver. Como os caminhos `pi.*`, `esp.*` e `calc.*` existem no simulador e nas duas placas, vão todas para o **comum**:

```toml
# joker.toml (acrescentar)
[instances.led_pi]
class = "std/led@1"
map   = { on = "pi.act_led" }

[instances.cpu]
class = "std/temp_sensor@1"
map   = { temperature = "calc.temp_media" }

[instances.led_esp]
class = "std/led@1"
map   = { on = "esp.led" }

[instances.botao]
class = "std/button@1"
map   = { pressed = "esp.button" }

[instances.toque]
class = "std/analog_in@1"
map   = { value = "calc.toque_nivel" }
```

- **Os nomes são do site.** Na 1.0, a instância implícita chama-se `esp` e é as três coisas ao mesmo tempo. O site quer falar de `botao` e de `led_esp`, e isso é uma composta (a ajuda do `J0807`).
- **Os nomes sobrevivem ao transporte.** Na parte 9, o ESP32 passa a Joker Micro e o dispositivo `esp` deixa de usar este perfil: o `joker-link` gera o seu a partir do firmware (ref/03 §2.2.16), e a instância implícita `esp` vai-se com o perfil. Os caminhos `esp.led` e `esp.button` ficam, e com eles as compostas, a `luz` e os cenários.
- **`toque` e `cpu` têm a calibração desta bancada.** O `esp` já é uma `analog_in@1`, com a escala nominal do firmware; mas "60 sem dedo, 15 com o dedo" é deste ESP32 e deste dedo, e fica no `calc.toque_nivel` da parte 5. É a regra do documento 14, §4.3: a normalização é do perfil ou da instância, e aqui cada um diz o que sabe. Do mesmo modo, o `cpu` aponta para `calc.temp_media` (`avg(pi.cpu_temp, 10s)`), que é o que a 0.4 lia: uma composta liga membros a **qualquer** caminho, e um `calc` é um sinal com a qualidade de quem o alimenta (documento 16, §3).
- **Nada do resto muda.** O `safe = false` de `pi.act_led` e de `esp.led`, os `calc` e os alarmes ficam onde estavam. O envelope declara-se por **um** dos caminhos de um sinal (`esp.led`, `esp.on` ou `led_esp.on`), nunca por dois (`J0215`); deixa-o no do sinal.

A instância implícita fica na mesma: é o que o perfil promete a quem o use, e é o que um `jk class verify` contra `esp` confirmaria antes de o publicares num registo.

## 6. A app, versão 0.5

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;
use joker::classes::std::{AnalogIn, Button, Led, TempSensor};

#[joker::app(name = "luz", version = "0.5.0")]
pub struct Luz {
    #[device] led: Led,              // led_pi
    #[device] led_estado: Led,       // led_esp
    #[device] temp: TempSensor,      // cpu
    #[device] botao: Button,         // botao
    /// 0 = sem dedo, 1 = dedo em cima.
    #[device] ajuste: AnalogIn,      // toque

    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_frio: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_quente: Param<f64>,
}

const RECUO_K: f64 = 15.0;
const FAIXA_K: f64 = 25.0;
const AJUSTE_NEUTRO: f64 = RECUO_K / FAIXA_K; // 0.6

#[joker::state(persist)]
struct State { ligado: bool, ajuste: f64, decorrido_ms: f64, aceso: bool }   // o `premido` saiu

impl Default for State {
    fn default() -> Self {
        State { ligado: true, ajuste: AJUSTE_NEUTRO, decorrido_ms: 0.0, aceso: false }   // arranca ligado
    }
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut State) -> Result<()> {
    // 1. Botão. O flanco é a consulta `clicked`, do núcleo. Sem qualidade é Err: não premido.
    if io.botao.clicked().unwrap_or(false) {
        st.ligado = !st.ligado;
        st.decorrido_ms = 0.0;
        st.aceso = st.ligado;
    }
    // O LED do ESP32 só se propõe quando se vê o ESP32 (a correção da 0.4.1).
    if io.botao.pressed().is_ok() {
        io.led_estado.set(st.ligado);
    }

    // 2. Ajuste. A classe não tem get_or; o membro cru tem, e a substituição fica no traço.
    st.ajuste = io.ajuste.raw().value.get_or(st.ajuste);

    if !st.ligado {
        io.led.off();
        return Ok(());
    }

    // 3. Temperatura. `?` → Err → o lease do led expira → safe (como na 0.2).
    let temp = io.temp.temperature()?;
    let t_frio = io.t_frio.get();
    let t_quente = io.t_quente.get() - RECUO_K + FAIXA_K * st.ajuste;
    let x = ((temp - t_frio) / (t_quente - t_frio).max(1.0)).clamp(0.0, 1.0);
    let meio_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);

    st.decorrido_ms += io.dt().as_secs_f64() * 1000.0;
    if st.decorrido_ms >= meio_ms {
        st.decorrido_ms -= meio_ms;
        st.aceso = !st.aceso;
    }
    io.led.set(st.aceso);                   // a cada tick: renova o lease
    Ok(())
}
```

```toml
# joker.toml (a [apps.luz] muda só no bind)
[apps.luz]
path   = "apps/luz"
bind   = { led = "led_pi", led_estado = "led_esp", temp = "cpu", botao = "botao", ajuste = "toque" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

Tudo o resto é a 0.4.1 linha a linha: as constantes, o `Default`, as contas do meio período. As portas têm os **mesmos nomes**; mudam os tipos (de `Input<bool>` para `Button`) e, no `bind`, os valores (de caminhos de sinais para nomes de instâncias). O `jk check` confirma que cada instância cumpre a classe que a porta pede, com a mesma major (`J0630`). O nome da porta é o **papel** na app (`led_estado`: o LED que mostra se está ligado); o nome da instância é o **equipamento** no site (`led_esp`).

Comparado com a 0.4.1:

| Saiu da app | Para onde foi |
|---|---|
| `premido` no estado e o `premido && !st.premido` | consulta `clicked` da `std/button@1`, calculada pelo núcleo |
| `botao.sample().quality.is_good()` | `botao.pressed().is_ok()`: ler um membro dá `Ok` só com qualidade `good` (ref/04 §4.8.2) |
| `botao.get_or(false)` | `clicked()` devolve `Err` quando `pressed` não está `good`; o `unwrap_or(false)` é a mesma regra: sem botão, não há flancos |
| `temp.get()?` | `temperature()?`: `Err` (`J-E0101`) sem qualidade `good`, como antes |
| `ajuste.get_or(st.ajuste)` | `raw().value.get_or(st.ajuste)`: a classe não tem `get_or`, o membro cru tem (documento 14, §3) |
| `led.set(false)` | `led.off()`, um método da classe |
| caminhos de sinais nas ligações | nomes de instâncias |

**Porque não `io.led.toggle()`?** Era a tentação, e seria um erro. Propor é renovar o lease (parte 1): um `toggle()` a cada meio período deixava a app centenas de milissegundos sem propor, o lease de 60 ms expirava, e o árbitro aplicava o `safe` entre duas mudanças. O `toggle()` serve para "inverter a pedido", como num clique, e não para um sinal que se mantém a cada tick. A app continua a guardar `aceso` e a chamar `set` sempre.

Os **testes da app** mudam, porque as portas mudaram de tipo: onde a 0.4 fazia `.input("temp", 70.0)`, a 0.5 faz `.device("temp", Sim::temp_sensor().constant(70.0))`, e as asserções leem membros (`h.device("led").member("on")`) em vez de portas (ref/04 §6.6). Os **cenários** não mudam, e isso é a secção seguinte.

## 7. A prova: o cenário da parte 5, sem alterações

```sh
$ git status --short sim/scenarios/
$ jk test --env sim
```

```
apps/luz (cargo test, componente wasm no wasmtime) ... 6 passed
sim/scenarios/pisca.scenario.toml (joker.sim.toml, 10 s virtuais) ...... passed
sim/scenarios/aquece.scenario.toml (joker.sim.toml) .................... passed
sim/scenarios/botao.scenario.toml (joker.sim.toml) ..................... passed
sim/scenarios/falha_esp.scenario.toml (joker.sim.toml) ................. passed
```

O `git status` não mostra nada: nenhum cenário foi tocado. O `falha_esp` desliga o ESP32 simulado aos 20 s, verifica que o `pi.act_led` continua a piscar a 200 ms, que o `esp_offline` dispara, que a pressão aos 30 s não conta e que, aos 41 s, o sistema ainda está ligado, e passa. Ele não sabe que a app mudou de camada, porque só olha para sinais, alarmes e qualidades do site. É o melhor teste de uma refatoração: **o comportamento observável é o mesmo**.

O `jk inspect apps/luz` mostra porquê: depois de expandidas as classes, a app tem as mesmas cinco portas da 0.4 (`led.on`, `temp.temperature`, `botao.pressed`, `led_estado.on`, `ajuste.value`) mais a consulta `botao.clicked`, e é essa lista plana que o host, o árbitro e o traço veem (documento 14, §3). O `jk inspect` lista os métodos e consultas que cada handle **conhece**, e não os que o código chama: isso pediria análise estática do Wasm (ref/04 §5 e §9).

## 8. No Pi 3 e no Zero

Nada muda no `joker.rpi.toml`, no `joker.lab.toml` nem no `joker.zero.toml`. As instâncias compostas estão no comum, e a implícita vem do perfil, que os dois nós usam (Modbus RTU no Pi 3, Modbus TCP no Zero). O zero está parado desde o início da parte: primeiro o lab.

```sh
$ jk check --env lab
```

```
✓ manifesto     joker.lab.toml ← joker.rpi.toml ← joker.toml · 2 dispositivos, 2 saídas, 1 app, 1 grupo
✓ instâncias    esp (std/led@1 + std/button@1 + std/analog_in@1, implícita pelo perfil local/bench-esp32@1)
                led_pi, led_esp (std/led@1) · botao (std/button@1) · cpu (std/temp_sensor@1) · toque (std/analog_in@1): compostas
✓ ligações      luz.led → led_pi · luz.led_estado → led_esp · luz.temp → cpu · luz.botao → botao · luz.ajuste → toque
nota: joker.lab.lock atualizado (profiles."local/bench-esp32@1" 1.1.0; classes std/led@1, std/button@1, std/analog_in@1, std/temp_sensor@1)
```

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
```

```
plano para lab (geração 7 → 8)
  perfil     local/bench-esp32@1   1.0.0 → 1.1.0   (só implements: o driver não reinicia)
  instâncias + esp (implícita) · + led_pi, + led_esp, + botao, + cpu, + toque (compostas)
  apps       luz 0.4.1 → 0.5.0     reinicia (portas mudaram de tipo)
  confirmação pedida: sem `jk confirm` em 5 min, volta à geração 7
```

```sh
$ jk confirm --node lab
$ jk status --instances --node lab
```

```
instância  classes                                      origem                              saúde
botao      std/button@1                                 manifesto (joker.toml:49)           Ok
cpu        std/temp_sensor@1                            manifesto (joker.toml:41)           Ok
esp        std/led@1 + std/button@1 + std/analog_in@1   perfil local/bench-esp32@1.1.0      Ok
led_esp    std/led@1                                    manifesto (joker.toml:45)           Ok
led_pi     std/led@1                                    manifesto (joker.toml:37)           Ok
toque      std/analog_in@1                              manifesto (joker.toml:53)           Ok
```

O LED `ACT` pisca como antes, o `BOOT` liga e desliga, o toque mexe no ritmo. A cadeia completa de uma instância:

```sh
$ jk explain botao --node lab
```

```
botao · std/button@1 · instância composta (joker.toml:49)
  pressed    in bool    botao.pressed → esp.button · perfil local/bench-esp32@1: discrete input 0 (FC 2)
                        dispositivo esp, modbus-rtu, unit 1 (joker.lab.toml:18) · debounce 20 ms (joker.toml:15)
                        poll 100 ms · false, good, idade 38 ms
  clicked    consulta   rose(pressed), no latch de main · lida por luz.botao
  released   consulta   fell(pressed) · sem leitores
  long_hold  consulta   held(pressed, 1 s) · sem leitores
  saúde      Ok
  também     esp.pressed (instância implícita esp) é o mesmo sinal
```

O `jk explain cpu --node lab` faz o mesmo: `cpu.temperature → calc.temp_media = avg(pi.cpu_temp, 10s) → sysfs thermal_zone0 (joker.rpi.toml)`, com o valor e a qualidade de cada elo; e o `jk explain esp` mostra as três classes da implícita, com as linhas do perfil que as mapeiam. É isto que o `jk explain` dá: cada membro até ao registo ou ao ficheiro do `sysfs`, a origem de cada linha (perfil, manifesto, overlay), as consultas e quem as lê.

No Zero é igual, com outro transporte. Troca de mestre (`lab$ sudo systemctl stop jokerd`, `zero$ sudo systemctl start jokerd`) e:

```sh
$ jk build --target rpi-zero-w
$ jk apply --env zero --node zero --confirm 5m && jk confirm --node zero
```

O `jk status --instances --node zero` dá a mesma lista, e o `jk explain botao --node zero` mostra `modbus-tcp` onde o Pi 3 mostrava `modbus-rtu`. O resto é igual. Volta ao lab para o resto da parte e para a parte 8 (`zero$ sudo systemctl stop jokerd`, `lab$ sudo systemctl start jokerd`).

## 9. Verificar uma classe no hardware

As classes da `std/` trazem casos de conformidade (`classes/std/led.conformance.toml`, no esquema da ref/03 §2.6) que qualquer instância tem de passar. Correr esses casos no `led_esp` prova que a instância, o perfil e o envelope do site fazem o que a classe promete, com o ESP32 a sério. No simulador corre-se sem pensar; no hardware, os casos acendem e apagam o LED a sério:

```sh
$ jk class verify std/led@1 --instance led_esp --env lab --node lab
```

```
error[J0640]: `jk class verify` numa instância não simulada sem `--allow-actuation`
  = nota: `led_esp` é modbus-rtu no nó lab: os casos vão atuar no equipamento
  = ajuda: --allow-actuation (só num nó com mode = "dev"; o escritor de teste passa pelo envelope do site)
```

Numa instância real, o escritor de teste não é o `control:12` do simulador: escreve como `manual:3`, com um TTL igual à duração da suite, e ganha às apps sem as parar. A `luz` continua a correr e fica `overridden(manual:3)` enquanto o teste dura. Se já houvesse um nível `safety` ou `manual` ativo nas saídas da instância (um `jk write` teu, por exemplo), o `verify` recusava com `J0641`: não se testa por cima de uma decisão de alguém.

```sh
$ jk class verify std/led@1 --instance led_esp --allow-actuation --env lab --node lab
```

```
std/led@1 · instância led_esp (composta: on → esp.led, modbus-rtu) · nó lab · mode dev
aviso: atuação real; escritor test:std/led@1 em manual:3, TTL 2,5 s (a duração da suite)
  on_sets_on ......................... ok
  off_clears_on ...................... ok
  toggle_inverts_own ................. ok
  lease_expiry_goes_safe ............. skipped (sim_only)
4 casos · 3 ok · 1 ignorado (sim_only) · 1,4 s
```

O último caso está marcado `sim_only = true` no `led.conformance.toml`: espera que, sem ninguém a mandar, o LED vá para o `safe`, e isso pede **posse exclusiva** da instância. Aqui a `luz` continua a propor por baixo do `manual:3`, e quando o lease do teste expira o `esp.led` volta ao valor da `luz`, e não ao `safe`. Em vez de o deixar falhar, o `verify` ignora-o e não o conta no código de saída (ref/03 §2.6.3, regra 6); por isso já não é preciso parar a `luz` para obter essa posse: o único caso que dela precisava não corre no hardware.

No simulador, o mesmo caso corre como os outros: `jk class verify std/led@1 --instance led_esp --env sim` dá `4 casos · 4 ok · 0 ignorados`. Aí o caso só passa porque o `safe` do site coincide com o `@safe_hint` da classe (`false`). Se o teu `esp.led` tivesse `safe = true`, seria ignorado com `J0846`: o `safe` é do site, e a suite não o pode presumir. Com `--instance esp`, os mesmos casos verificam o mapeamento do próprio perfil, e com `--report bench-esp32.cbor` guardam o relatório que a publicação de um perfil com `implements` exige (ref/03 §2.8).

## 10. Trocar os LEDs: uma linha

Queres que o LED do ESP32 pisque e o do Pi mostre o estado? Ou tens uma placa ESP32 sem LED no GPIO2 (algumas não têm, documento 23 §3)? A app não sabe de que LED se trata: sabe que tem um que pisca e um que mostra o estado. Troca-se a linha do `bind`:

```toml
bind = { led = "led_esp", led_estado = "led_pi", temp = "cpu", botao = "botao", ajuste = "toque" }
```

Com o `jk dev --env sim --panel` a correr, grava: a lâmpada do ESP32 começa a piscar e a do Pi fica acesa fixa. Nenhuma linha de Rust mudou, nenhum envelope mudou (o `esp.led` já tinha `safe`), e o `jk check` não tem nada a dizer.

Agora corre `jk test`: o `falha_esp` **falha**, com `J0842` no `pi.act_led`. E deve falhar. O cenário verifica o comportamento do **site** ("com o ESP32 em baixo, o Pi continua a piscar"), e tu mudaste o comportamento do site, embora não tenhas mudado a app. Volta a pôr a linha como estava.

## 11. O que aconteceu por baixo

- **As classes resolvem-se no build e no `jk check`** (documento 14, A12). Cada membro é uma porta; cada método é um registo de ~16 B no buffer de propostas, que o núcleo expande no passo 5a do tick (documento 17, §4.1); cada consulta é um sinal derivado. Não há dispatch dinâmico no tick.
- **A instância implícita vem do perfil, e a sua forma é fixa na 1.0**: um dispositivo, uma instância, com o nome do dispositivo e as classes que o perfil declara (ref/03 §2.2.14). Os `.contract.json` das classes vão dentro da geração, e o lock fixa a versão e o hash de contrato de cada uma (ref/02 §2.1.6; `J0422` se o hash mudar sem a versão).
- **A qualidade continua a ser do sinal.** `clicked()` e `temperature()` devolvem `Err` pela mesma razão que o `get()` da 0.4: a entrada não está `good`. A saúde de uma instância (`health()`, documento 17 §4.5) junta a dos membros obrigatórios, sem inventar médias. O `jk trace pi.act_led` continua a mostrar `app:luz.pisca` a propor: as ligações mudaram de camada, os sinais não.

## 12. Exercícios

1. Usa `io.botao.long_hold()` para que uma pressão de 1 s desligue sempre o sistema, esteja ele ligado ou não. Com o código atual, uma pressão longa também dispara `clicked` no início: como é que evitas que ela ligue o sistema antes de o desligar? Escreve primeiro o teste, com `Sim::button().press_at(1.s(), 1200.ms())`.
2. Muda `ajuste = "toque"` para `ajuste = "esp"` no `bind` (a implícita também é uma `analog_in@1`). O que muda no ritmo do pisca quando tocas? Porquê? Qual das duas normalizações devia ir para um perfil publicado?
3. Muda o `cpu` para `temperature = "pi.cpu_temp"` e corre o `aquece.scenario.toml`. O que acontece ao ritmo do LED, e que parte do cenário falha?
4. Desliga o cabo USB do ESP32 com o sistema a correr no Pi 3 e olha para o `jk status --instances --node lab` e para o `jk explain botao`. Que membros ficam `bad(comm_failure)`, e o que continua `Ok`?

```sh
$ git add -A && git commit -qm "trilha(07): luz 0.5 sobre classes; implements no perfil do ESP32; instâncias compostas"
```

**Estado no fim desta parte:** app `luz` **0.5**, no lab (geração 8) e no zero (geração 2, parado). Instâncias: `led_pi`, `led_esp`, `botao`, `cpu`, `toque` (compostas, no `joker.toml`) e `esp` (implícita, pelo perfil). Ficheiros alterados: `joker.toml` (as cinco `[instances.*]` e o `bind` da `luz`), `profiles/local/bench-esp32@1.profile.toml` (três `[implements]`, versão 1.1.0), `sim/esp.sim.toml` (os mesmos `[implements]`), `apps/luz/`, `joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`. Os cenários em `sim/scenarios/` não mudaram.

**Próximo:** [Parte 8 — A tua classe: `status_light@1`](08-your-own-class.md).
