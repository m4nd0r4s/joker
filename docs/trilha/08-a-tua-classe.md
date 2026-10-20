# Parte 8 — A tua classe: `status_light@1`

**Objetivo:** tirar da `luz` a última coisa que não é negócio (o pisca), transformá-la numa classe tua, `local/status_light@1`, com contrato, casos de conformidade e uma implementação reutilizável, e acabar com uma app de negócio de ~20 linhas que se lê como a especificação do sistema.

**Vais aprender:** `jk class new` · a IDL `.jkc`: enums, `out`, métodos, métodos atómicos, consultas · o ficheiro de conformidade no esquema único · instâncias **virtuais** e `#[joker::implements]` · comandos virtuais com envelope e `safe` · `order` no grupo · `jk class verify` no simulador e no Pi 3 · segurança em cada camada · as quatro pessoas do documento 14.

**Antes de começar:** a parte 7 feita: `luz` **0.5** sobre classes, as instâncias compostas `led_pi`, `led_esp`, `botao`, `cpu`, `toque` e a implícita `esp`, o `falha_esp` a passar. O Pi 3 com o ESP32 por USB.

---

## 1. O cheiro

Relê a `pisca` da 0.5. Metade continua a ser sobre **como** mostrar um estado com um LED: `aceso`, `decorrido_ms`, subtrair o meio período, propor a cada tick para não perder o lease. Nada disto é sobre temperatura. Se amanhã outro projeto precisar de uma luz que pisca, vai copiar estas linhas, e cada cópia vai divergir.

A pergunta de desenho: **o que é que a app de negócio quer dizer à luz?** Quatro coisas: apagada, acesa, a piscar com um período, e alerta. Isso é um contrato, e um contrato é uma classe.

## 2. Criar a classe

```sh
$ jk class new local/status_light@1
```

```
criado classes/local/status_light.jkc
criado classes/local/status_light.conformance.toml
nota: o pacote `local` é do projeto: não tem `.package.toml` e nunca se publica (ref/03 §2.7)
```

O ficheiro de conformidade tem o **mesmo nome** do `.jkc` (`status_light.jkc` ↔ `status_light.conformance.toml`, ref/03 §2.7). Escreve a classe:

```
// classes/local/status_light.jkc
package local

/// O que uma luz de estado está a mostrar.
enum LightMode { off = 0, on = 1, blink = 2, alert = 3 }

/// Luz de estado: apagada, acesa, a piscar com um período, ou em alerta.
/// Diz o quê, e não como: a implementação pode ser um LED, um sinalizador ou um ecrã.
class status_light@1.0.0 {
    /// O modo pedido.
    out mode: LightMode              @safe_hint(LightMode.off)

    /// Período completo do pisca, usado no modo `blink`.
    out blink_period: f64 ms         @safe_hint(1000 ms) @range_hint(40 ms, 10000 ms)

    method off()   { mode = LightMode.off }
    method on()    { mode = LightMode.on }
    method alert() { mode = LightMode.alert }

    atomic method blink(period: f64 ms) order(blink_period, mode) {
        blink_period = clamp(period, 40 ms, 10000 ms)
        mode         = LightMode.blink
    }

    /// A luz está a pedir atenção.
    query alerting: bool = mode == LightMode.alert
}
```

As decisões são tuas, como autor da classe, e cada uma tem uma razão no documento 17:

- **`blink(period)` não é uma variante do enum.** Um enum da IDL é uma lista de nomes com números, sem dados (17 §3.3), porque os `in`/`out` são sempre escalares ou enums. O período é um membro à parte, e o método escreve os dois.
- **O método é `atomic`, com `order(blink_period, mode)`.** O período e o modo chegam juntos à implementação: nunca há um tick com `blink` e o período antigo (M20; documento 14, A5). Sem `order`, é `JC0205`.
- **O modo atual não precisa de uma consulta.** Ler um `out` dá o valor **aplicado**, depois do árbitro e do envelope (17 §11): `io.sinal.mode()` numa app, `jk read sinal.mode` na CLI. A consulta `alerting` é outra coisa: um derivado que um alarme, uma HMI ou uma ponte podem usar sem conhecer o enum.
- **Só saídas, e nada sobre LEDs.** Quem usa a luz diz-lhe o que mostrar, e não lhe pergunta nada (uma bomba, com retorno, teria `in`). Não há `on: bool`, pinos, nem o desenho do alerta: isso é da implementação.
- **`@safe_hint` e `@range_hint` são sugestões.** O `safe` é do site (ADR 0006), e vais declará-lo na secção 5.

### O que o `jkc check` diz

A primeira versão que toda a gente escreve chama ao membro `period`, como ao argumento. O `jk class check classes/local/status_light.jkc` recusa-a:

```
erro[JC0206]: o argumento `period` tem o nome de um membro da classe
 --> classes/local/status_light.jkc:17:25
   |
17 |     atomic method blink(period: f64 ms) order(period, mode) {
   |                         ^^^^^^ membros, argumentos e consultas partilham o espaço de nomes da classe
   = ajuda: muda o nome do membro (`blink_period`) ou do argumento
```

Com os nomes da versão acima: `✓ local/status_light@1.0.0 · 2 membros · 4 métodos (1 atómico) · 1 consulta · 1 enum · contrato sha256:5b0e…`.

A partir daqui, `jk class show local/status_light@1` mostra a tua classe como mostrava a `std/button@1`, e o `build.rs` das apps gera `crate::classes::local::{StatusLight, LightMode}` (ref/04 §4.8.1).

## 3. O contrato de conformidade

Uma classe só de saídas não tem retorno para observar. Os casos verificam o **contrato**: que os métodos se expandem no que prometem, que o `blink` é atómico e limitado, e que, sem ninguém a mandar, a luz vai para o `safe`. O pisca em si testa-se nos testes da implementação (secção 4), e não aqui (ref/03 §2.6.5).

```toml
# classes/local/status_light.conformance.toml
schema = 1

[suite]
kind        = "class"
class       = "local/status_light@1"
description = "contrato: expansão dos métodos, blink atómico e limitado, safe depois do lease"

[[case]]
id     = "on_sets_mode"
do     = [{ at = "100ms", method = "on" }]
expect = [{ member = "mode", equals = "on", at = "100ms" }]

[[case]]
id     = "blink_is_atomic_and_clamped"
do     = [{ at = "100ms", method = "blink", args = { period = "10 ms" } }]
expect = [{ all_of = [{ member = "blink_period", equals = "40 ms", at = "100ms" },
                      { member = "mode", equals = "blink", at = "100ms" }] }]

[[case]]
id     = "alert_raises_query"
do     = [{ at = "100ms", method = "alert" }]
expect = [{ query = "alerting", equals = true, within = "100ms .. 200ms" }]

[[case]]
id       = "lease_expiry_goes_off"
sim_only = true          # precisa de posse exclusiva: numa instância real, a app continua a propor
do       = [{ at = "100ms", method = "on" }, { at = "1s", lease_expire = "mode" }]
expect   = [{ member = "mode", equals = "off", level = "none", within = "1s .. 1.1s" }]
```

É o **mesmo esquema** dos casos do núcleo e dos drivers (`[suite]`, `[[case]]`, `id`, `do`, `expect`); só a *fixture* muda (ref/03 §2.6). O executor é um escritor único, `test:local/status_light@1`, que mantém as suas propostas em cada tick como faria uma app. Os instantes são múltiplos do período do grupo dono da instância (`main`, 20 ms). O último caso tem duas condições. Está marcado `sim_only = true`: espera `level = "none"`, e isso só se vê com **posse exclusiva** da instância, que só a simulação dá; numa instância real, as apps continuam a propor por baixo do escritor de teste, e o `verify` reporta-o `skipped (sim_only)` (ref/03 §2.6.3, regra 6). E só corre se o `safe` do site coincidir com o `@safe_hint`: senão, é ignorado com `J0846`, porque o `safe` é do site e a suite não o pode presumir.

## 4. A implementação: `status_light_ctl`

A implementação recebe os **comandos** da classe (`io.cmd.mode`, `io.cmd.blink_period`: os valores aplicados, depois do árbitro e do envelope) e usa um `Led` para os mostrar. É para aqui que vai o código que sai da `luz`.

```sh
$ jk app new status_light_ctl --lang rust
```

```rust
// apps/status_light_ctl/src/lib.rs
use joker::prelude::*;
use joker::classes::std::Led;
use crate::classes::local::{LightMode, StatusLight};   // gerados pelo build.rs a partir de classes/local/

/// Implementa local/status_light@1 sobre um LED.
#[joker::app(name = "status_light_ctl", version = "1.0.0")]
#[joker::implements(StatusLight)]
pub struct StatusLightCtl {
    #[device] led: Led,
}

#[derive(Default)]
struct Padrao {
    modo: Option<LightMode>,
    t_ms: f64,        // tempo desde o início do padrão atual
    aceso: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn run(io: &mut CycleIo<StatusLightCtl>, st: &mut Padrao) -> Result<()> {
    let modo = io.cmd.mode.get()?;                   // sem escritor, o comando está no safe: off
    if st.modo != Some(modo) {                       // mudou de modo: o padrão recomeça, e o pisca começa aceso
        *st = Padrao { modo: Some(modo), aceso: modo == LightMode::Blink, ..Padrao::default() };
    }
    st.t_ms += io.dt().as_secs_f64() * 1000.0;

    st.aceso = match modo {
        LightMode::Off => false,
        LightMode::On => true,
        LightMode::Blink => {
            let meio = io.cmd.blink_period.get()? / 2.0;
            if st.t_ms >= meio { st.t_ms -= meio; !st.aceso } else { st.aceso }
        }
        LightMode::Alert => {
            let t = st.t_ms % 1000.0;                // duplo flash por segundo
            t < 100.0 || (200.0..300.0).contains(&t)
        }
    };
    io.led.set(st.aceso);                            // a cada tick: renova o lease do LED
    Ok(())
}
```

O `#[joker::implements(StatusLight)]` gera `io.cmd`, com um handle de entrada por cada `out` da classe, e `io.state`, que aqui está vazio porque a classe não tem `in` (ref/04 §4.8.4). Um nome da classe que não exista, ou uma capacidade que ela não tenha, é `J-E0715` no build.

A implementação tem os seus próprios testes, independentes de quem a usa:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn blink_de_200ms_alterna_a_cada_100ms() {
        let mut h = Harness::<StatusLightCtl>::new()
            .group("run", 20.ms())
            .device("led", Sim::of::<Led>())
            .command("mode", LightMode::Blink)
            .command("blink_period", 200.0);
        h.run_for(2.s());
        assert!(h.device("led").member("on").after(200.ms()).toggles_every(100.ms()));
    }
}
```

O `.command(…)` dá à implementação um comando virtual, como se alguém o tivesse mandado (ref/04 §6.2). Um segundo teste, para o `alert`, conta 4 mudanças por segundo (`between(500.ms(), 2500.ms()).toggles() == 8`).

## 5. O manifesto: uma instância virtual

```toml
# joker.toml (acrescentar)
[instances.sinal]
class          = "local/status_light@1"        # só o std/ se pode omitir
implemented_by = "app:status_light_ctl"        # instância virtual

# Os comandos virtuais são saídas como as outras: têm árbitro, envelope e safe.
[outputs."sinal.mode"]
safe = "off"

[outputs."sinal.blink_period"]
safe = "1000 ms"

[apps.status_light_ctl]
path   = "apps/status_light_ctl"
bind   = { led = "led_pi" }
cycles = { run = "main" }
```

```toml
# joker.toml (a [apps.luz] e o [cycles.main] mudam)
[apps.luz]
path   = "apps/luz"
bind   = { sinal = "sinal", led_estado = "led_esp", temp = "cpu", botao = "botao", ajuste = "toque" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }

[cycles.main]
period = "20ms"
order  = ["luz", "status_light_ctl"]           # quem manda antes de quem executa
```

- **`implemented_by = "app:status_light_ctl"`** usa o `bind`, os `params` e os `cycles` de `[apps.status_light_ctl]`, e serve uma instância (ref/01 §5.13). Para três luzes com a mesma implementação, a forma em tabela (`{ app, bind }`) cria uma instância da app por instância virtual (documento 17, §5.3).
- **O `pi.act_led` tem agora outro escritor**: a `status_light_ctl`, pela instância `led_pi`. A `luz` já não lhe toca. Cada saída continua com um só escritor, e o `writer` por omissão chega.
- **`order`.** Sem ele, as tarefas correm por ordem lexicográfica de `<app>.<tarefa>` (ref/01 §5.11), que por acaso também põe a `luz` primeiro. Com ele, a ordem é uma decisão escrita, e não um acaso dos nomes: o núcleo arbitra e aplica o envelope aos comandos virtuais logo a seguir à `luz`, e a `status_light_ctl` lê-os no mesmo tick. A latência entre as camadas fica nula; ao contrário, seria um tick de 20 ms (documento 14, §7.2). Uma entrada de `order` que não corre no grupo é `J0281`.

### O que o `jk check` diz sem o `safe`

Apaga o `[outputs."sinal.blink_period"]` e verifica:

```sh
$ jk check --env sim
```

```
erro[J0201]: a saída `sinal.blink_period` não tem `safe`
 --> joker.toml:61:1
   |
61 | [instances.sinal]
   | ^^^^^^^^^^^^^^^^^ instância virtual: cada `out` da classe é um comando com envelope
   = nota: sugestão da classe local/status_light@1: @safe_hint(1000 ms)
   = ajuda: [outputs."sinal.blink_period"] safe = "1000 ms"
```

A classe sugere, o site decide. Repõe a tabela.

## 6. A app de negócio, versão 0.6

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;
use joker::classes::std::{AnalogIn, Button, Led, TempSensor};
use crate::classes::local::StatusLight;

#[joker::app(name = "luz", version = "0.6.0")]
pub struct Luz {
    #[device] sinal: StatusLight,    // o que mostrar, e não como
    #[device] led_estado: Led,
    #[device] temp: TempSensor,
    #[device] botao: Button,
    #[device] ajuste: AnalogIn,
    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_frio: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_quente: Param<f64>,
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut State) -> Result<()> {
    if io.botao.clicked().unwrap_or(false) {
        st.ligado = !st.ligado;
    }
    if io.botao.pressed().is_ok() {
        io.led_estado.set(st.ligado);
    }
    st.ajuste = io.ajuste.raw().value.get_or(st.ajuste);
    if !st.ligado {
        io.sinal.off();
        return Ok(());
    }
    let temp = io.temp.temperature()?;           // sem temperatura: o lease do sinal expira → off
    let t_frio = io.t_frio.get();
    let t_quente = io.t_quente.get() - RECUO_K + FAIXA_K * st.ajuste;
    let x = ((temp - t_frio) / (t_quente - t_frio).max(1.0)).clamp(0.0, 1.0);
    let meio_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);
    io.sinal.blink(2.0 * meio_ms);
    Ok(())
}
```

As constantes `RECUO_K`, `FAIXA_K` e `AJUSTE_NEUTRO` e o `Default` do `State` são os da 0.5; o `State` perdeu `decorrido_ms` e `aceso`, que foram para o `Padrao` da implementação, e fica `State { ligado, ajuste }`. Lê-se como a especificação da parte 5: *sem temperatura, nada; um clique liga e desliga; o LED do ESP32 mostra se está ligado; o toque ajusta o `t_quente`; ligado, pisca com o meio período da temperatura.* **É isto a camada 3.**

Repara que `blink()` e `off()` são chamados a cada tick. Propor o mesmo valor renova o lease do comando virtual, e o núcleo não manda nada à implementação quando o valor aplicado não muda. Se a `luz` morrer, o lease de `sinal.mode` expira e a luz vai para `off`.

Os testes de negócio já não falam de LEDs: com `.device("sinal", Sim::of::<StatusLight>())`, a 70 °C e ajuste 0,6 (`t_quente` = 70 °C), verificam que `h.device("sinal").member("mode").last()` é `Some(LightMode::Blink)` e que o `blink_period` é 200 ms. O pisca propriamente dito é testado uma vez, na implementação.

## 7. No simulador

```sh
$ jk test --env sim
```

```
apps/luz (cargo test, componente wasm no wasmtime) ............... 5 passed
apps/status_light_ctl (cargo test, componente wasm no wasmtime) .. 2 passed
sim/scenarios/pisca.scenario.toml (joker.sim.toml) .............................. FAILED
  error[J0842]: 8.1s pi.act_led: level esperado "none", visto "control:12" (app:status_light_ctl)
sim/scenarios/{aquece,botao}.scenario.toml (joker.sim.toml) ..................... 2 passed
sim/scenarios/falha_esp.scenario.toml (joker.sim.toml) .......................... passed
```

O `falha_esp` continua a passar, agora com **duas** apps em camadas entre a `luz` e o `pi.act_led`. Visto de fora, o comportamento é o mesmo desde a 0.4.

O `pisca.scenario.toml` da parte 1 falha, e com razão. Dizia "a `luz` para, e o LED vai para o `safe` sem ninguém a escrever" (`level = "none"`). Agora a `luz` já não escreve no LED: quando para, é o lease de `sinal.mode` que expira, o `safe` aplica-se **uma camada acima** (`off`), e a `status_light_ctl` apaga o LED em `control:12`. O LED apaga-se na mesma; mudou quem o apaga. Corrige a expectativa para dizer isso, e não a app:

```toml
# sim/scenarios/pisca.scenario.toml (a última expectativa passa a duas)
[[expect]]
at     = "8.1s"
signal = "pi.act_led"
equals = false

[[expect]]
at     = "8.1s"
signal = "sinal.mode"
equals = "off"
level  = "none"                     # o safe do comando virtual: ninguém manda na luz
```

Agora o contrato, contra a instância real do site (em simulação):

```sh
$ jk class verify local/status_light@1 --instance sinal --env sim
```

```
local/status_light@1 · instância sinal (virtual, app:status_light_ctl) · joker.sim.toml · relógio virtual
  on_sets_mode ...................... ok
  blink_is_atomic_and_clamped ....... ok
  alert_raises_query ................ ok
  lease_expiry_goes_off ............. ok   (safe = off no site, igual ao @safe_hint)
4 casos · 4 ok · 0 ignorados · 1,1 s simulados em 22 ms
```

Para uma instância virtual, o `verify` corre os casos contra a app que a implementa, com o núcleo real e relógio virtual (ref/04 §6.7). Quem mais escreve na instância (a `luz`) não entra.

E a cadeia inteira:

```sh
$ jk explain sinal --env sim
```

```
sinal · local/status_light@1 · instância virtual, implementada por app:status_light_ctl (joker.toml:61)
  mode           out LightMode   escritores: luz (control:12) · safe = off (joker.toml:66) · safe_hint off ✓
  blink_period   out f64 ms      escritores: luz (control:12) · safe = 1000 ms (joker.toml:69) · safe_hint 1000 ms ✓
  grupo atómico  blink → (blink_period, mode)
  alerting       consulta        mode == LightMode.alert
  latência       luz → status_light_ctl: 0 ticks (main, order declarada)
  implementação  status_light_ctl.led → led_pi (std/led@1) → pi.act_led → sim (portátil) / sysfs ACT (placas)
```

## 8. No Pi 3

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
```

```
plano para lab (geração 8 → 9)
  classes    + local/status_light@1 1.0.0 (contrato sha256:5b0e…)
  instâncias + sinal (virtual) · saídas + sinal.mode, + sinal.blink_period
  apps       luz 0.5.0 → 0.6.0 reinicia · + status_light_ctl 1.0.0 · main: order = [luz, status_light_ctl]
```

Depois do `jk confirm --node lab`, o LED `ACT` pisca como antes. O `joker.lab.toml` não mudou: a classe, a instância, as saídas e as duas apps estão no comum.

**O contrato no hardware.** Os mesmos casos, agora com o LED real. O escritor de teste escreve em `manual:3`, e a `luz` continua a correr, `overridden(manual:3)` enquanto a suite dura (como na parte 7):

```sh
$ jk class verify local/status_light@1 --instance sinal --allow-actuation --env lab --node lab
```

```
local/status_light@1 · instância sinal (virtual, app:status_light_ctl) · nó lab · mode dev
aviso: atuação real; escritor test:local/status_light@1 em manual:3 (a luz fica overridden)
  on_sets_mode ...................... ok
  blink_is_atomic_and_clamped ....... ok
  alert_raises_query ................ ok
  lease_expiry_goes_off ............. skipped (sim_only)
4 casos · 3 ok · 1 ignorado (sim_only)
```

Não é preciso parar a `luz`: o único caso que pedia posse exclusiva da instância, o `lease_expiry_goes_off`, está marcado `sim_only` e não corre aqui (no simulador, correu e passou). Durante esses segundos, o LED `ACT` acende fixo, pisca muito depressa e faz o duplo flash; no fim da suite, o `manual:3` liberta-se e o LED volta ao pisca da `luz`. É a implementação a responder ao contrato, e não o teste a mexer no LED. O "sem ninguém a mandar, apaga" vê-se no hardware na primeira experiência a seguir.

**Partir as camadas uma a uma.** Três experiências que mostram que cada camada é segura por si (documento 14, §7.2):

1. **Para a app de negócio:** `jk app stop luz --node lab`. O lease de `sinal.mode` expira, o árbitro aplica o `safe` (`off`), e a `status_light_ctl` apaga o LED. Quem implementa não precisa de saber que quem manda morreu. `jk app start luz --node lab`.
2. **Para a implementação:** `jk app stop status_light_ctl --node lab`. Agora é o lease do `pi.act_led` que expira, e o LED apaga-se pelo `safe` da camada 1. A `luz` continua a mandar `blink`, mas ninguém executa, e o `jk status --instances --node lab` mostra `sinal … implementação parada`. `jk app start status_light_ctl --node lab`.
3. **Mandar na camada do meio.** Força o alerta durante um minuto, para ver se o LED se distingue do outro lado da sala:

```sh
$ jk call sinal.alert --ttl 1m --reason "teste de visibilidade" --node lab
```

O método é chamado como escritor manual (`manual:4`, que ganha ao `control:12` da `luz`) e expandido pelo núcleo como qualquer outro (documento 17, §4.1). O LED faz o duplo flash **verdadeiro**, gerado pela implementação, e passado um minuto volta ao pisca da `luz`. Um `jk write pi.act_led true` só te daria um LED aceso.

## 9. O que aconteceu por baixo

- **A classe é um contrato compilado.** O `jkc` gerou o `status_light@1.0.0.contract.json`, com o hash de contrato que o lock fixa (`[classes."local/status_light@1"]`, ref/02 §2.1.6), e os tipos Rust (`StatusLight`, `LightMode`, `StatusLightCmd`) que as duas apps importam (documento 17, §6). Mudar o corpo de um método ou tirar um membro exige a major 2 (17 §9; `JC0601` se não subires).
- **Um método é um registo, e não uma chamada.** `io.sinal.blink(…)` põe `(sinal, blink, args)` no buffer de propostas; no passo 5a do tick, o núcleo avalia as atribuições e cria duas propostas num grupo atómico, com a ordem do `order(…)` (documento 17, §4.1). A `jk call` passa pelo mesmo caminho.
- **Os comandos virtuais são saídas.** Têm árbitro, envelope e `safe`, e duas apps a mandar na mesma luz resolvem-se **antes** de chegarem à implementação. Com o `order`, a arbitragem dos comandos acontece logo a seguir à tarefa que os produz (documento 01, §4, passo 4b). `luz` → `sinal` → `led_pi` é uma camada virtual; o limite é 3 (`J0620`).

## 10. Quem fez o quê: as quatro pessoas, na tua bancada

Sem dares por isso, fizeste os papéis todos do documento 14, §1. Numa equipa, são normalmente pessoas diferentes, e cada uma só precisa de conhecer a sua camada e o contrato da de baixo.

| Quem | Na tua pasta | Precisa de saber | Não precisa de saber |
|---|---|---|---|
| **Quem integra** (integrador de hardware) | `joker.lab.toml`, `joker.zero.toml`, `joker.rpi.toml`; o perfil `local/bench-esp32@1` com os seus `[implements]`; as instâncias compostas `led_pi`, `led_esp`, `botao`, `cpu`, `toque` | registos, `sysfs`, calibrações, `safe` | a lógica da `luz` |
| **Quem cria abstrações** (autor de abstrações) | `classes/local/status_light.jkc` e o seu `.conformance.toml` | o que os programas querem dizer a uma luz | que há um LED `ACT` por baixo |
| **Quem implementa** (também autor de abstrações, no documento 14) | `apps/status_light_ctl/` e os seus testes | a classe que implementa e a classe que usa (`std/led@1`) | quem a vai mandar, e porquê |
| **Quem usa** (programador de negócio; e o operador, que só lê e manda por métodos) | `apps/luz/` 0.6; `jk status --instances`, `jk explain`, `jk call sinal.alert` | temperatura, cliques, o que a luz deve mostrar | registos, pinos, protocolos, padrões de pisca |

O documento 14 põe "quem implementa" dentro do autor de abstrações (uma classe e as suas implementações ativas costumam andar juntas) e dá ao operador um papel próprio. Na bancada vale a pena separá-los, porque a `status_light_ctl` é a única peça que conhece as duas classes: é a costura entre a camada 2 e a camada 1.

**Fim do Módulo B.** Tens a camada 1 dominada (Módulo A), e a camada 2 usada e criada: classes padrão, instâncias implícitas, compostas e virtuais, e uma classe tua com contrato, conformidade e implementação. A camada 3 diz o que o sistema faz, e mais nada.

## 11. Exercícios

1. Acrescenta um método `blink_once()` à classe (um flash de 200 ms, e depois `off`). Consegues escrevê-lo só com atribuições? Se não, onde vive esse comportamento? Seria um minor ou um major da `status_light@1`?
2. Escreve uma segunda implementação, `status_light_duo`, que mostra o modo nos **dois** LEDs (`on` no do Pi, `alert` a piscar os dois alternados). Troca só o `implemented_by` e o `bind` da nova app. A `luz` muda? E o `falha_esp`, passa? Porquê?
3. Tira o `order` do `[cycles.main]` e muda o nome da app para `zz_luz`. O que diz o `jk explain sinal` sobre a latência? Que caso de conformidade ou cenário o apanharia, se algum?
4. Acrescenta à `luz` o modo `alert` quando o alarme `cpu_quente` está ativo, com uma porta de camada 1 ligada a `alarm.cpu_quente.active`. É a versão 0.7 da parte 11: escreve primeiro o teste.
5. (Com o extra da secção 12.) Escreve um caso em `bench_io.conformance.toml` que verifica que `online` fica falso quando o `uptime` deixa de ser `good`. É um caso `sim_only`? Porquê?

```sh
$ git add -A && git commit -qm "trilha(08): classe local/status_light@1, status_light_ctl, luz 0.6"
```

## 12. Extra: o ESP32 inteiro numa classe

Opcional, uns 20 minutos, só no simulador. As partes seguintes não o assumem, e por isso faz-se num ramo à parte, que se deixa no fim.

```sh
$ git switch -c extra-bench-io
$ jk class new local/bench_io@1
```

```
criado classes/local/bench_io.jkc
criado classes/local/bench_io.conformance.toml
```

Até aqui, o ESP32 chega às apps como três instâncias soltas (`led_esp`, `botao`, `toque`) e dois sinais crus (`esp.rssi`, `esp.uptime`). Uma **classe composta** junta-os num só equipamento lógico, com membros que são outras classes:

```
// classes/local/bench_io.jkc
package local

/// O nó de I/O da bancada: um LED de estado, um botão, um toque, e se está vivo.
/// Descreve o papel (o nó de I/O), e não o chip: outro nó com as mesmas peças cumpre-a.
class bench_io@1.0.0 {
    member status: std/led@1
    member boot:   std/button@1
    member knob:   std/analog_in@1

    /// Intensidade do Wi-Fi, em dBm; 0 sem ligação.
    in rssi:   i64
    /// Tempo desde o arranque do nó.
    in uptime: f64 s

    query online: bool = good(uptime)

    health required(uptime)
}
```

- **O nome é o papel.** `bench_io` diz para que serve no programa; uma classe `esp32@1` diria a peça, e mentia no dia em que o nó fosse outro (documento 14, §2).
- **Os membros `member` são classes**, e não sinais. A profundidade é 2 (`bench_io` → `led`), abaixo do máximo de 3 (`JC0501`, documento 17, §8).
- **A saúde junta as duas coisas** (documento 17, §4.5): `Failed` se o `uptime` não estiver `good`, e a pior das saúdes de `status`, `boot` e `knob`, que são obrigatórios. O `rssi` não conta: na parte 9, sem Wi-Fi, fica `Bad(NotConnected)`, e a bancada continua `Ok`.

O `.conformance.toml` fica com o esqueleto: o `local` nunca se publica (ref/03 §2.7), e o exercício 5 pede-te um caso.

**A instância.** Os membros de classe ligam-se ao **nome de uma instância** que já existe; os escalares, a sinais, como sempre (ref/01 §5.13):

```toml
# joker.toml (acrescentar, no ramo)
[instances.esp_io]
class = "local/bench_io@1"
map   = { status = "led_esp", boot = "botao", knob = "toque", rssi = "esp.rssi", uptime = "esp.uptime" }
```

O `jk check` verifica que `led_esp`, `botao` e `toque` existem e são das classes certas (`J0650`, `J0651`). As instâncias continuam lá, com os seus nomes: `esp_io.boot.clicked` e `botao.clicked` são o **mesmo sinal**, e `esp_io.status.on`, `led_esp.on` e `esp.led` também (documento 16, P6).

**Uma app de diagnóstico.**

```rust
// apps/bench_diag/src/lib.rs
use joker::prelude::*;
use crate::classes::local::BenchIo;

#[joker::app(name = "bench_diag", version = "0.1.0")]
pub struct BenchDiag {
    #[device] bancada: BenchIo,
    #[publish(unit = "1")] cliques: Counter,
}

#[joker::cycle(period = "20ms", wcet = "10us")]
fn run(io: &mut CycleIo<BenchDiag>, _st: &mut ()) -> Result<()> {
    if !io.bancada.online()? {
        return Ok(());                                       // sem ESP32, não propõe nada
    }
    if io.bancada.boot.clicked()? {                          // handle aninhado: o botao, pela bancada
        io.cliques.inc();                                    // app.bench_diag.cliques
    }
    io.bancada.status.set(io.bancada.knob.value()? > 0.5);   // o LED do ESP32 segue o toque
    Ok(())
}
```

`io.bancada.boot` é um handle de `Button` como o da `luz` (ref/04 §4.8.6): os membros de classe são campos, e o resto é a API de sempre.

```toml
# joker.toml (acrescentar, no ramo)
[apps.bench_diag]
path   = "apps/bench_diag"
bind   = { bancada = "esp_io" }
cycles = { run = "main" }
writer = { level = "control", priority = 14 }     # abaixo da luz, que está em control:12
```

### O que o `jk check` diz

Sem o `writer`, as duas apps ficavam em `control:12` na mesma saída, o `esp.led`: é o `J0210` de sempre. Com ele, passa, com um aviso:

```sh
$ jk check --env sim
```

```
aviso[J0655]: a instância `led_esp` chega a duas apps por caminhos diferentes
 --> joker.toml:82:1
   |
82 | [instances.esp_io]
   | ^^^^^^^^^^^^^^^^^^ esp_io.status → led_esp (std/led@1) · out on → esp.led
   = nota: luz liga led_esp diretamente (led_estado), em control:12
   = nota: bench_diag chega-lhe por esp_io.status, em control:14
   = ajuda: é o mesmo `esp.led`, com um só envelope e um só árbitro; a luz ganha enquanto propuser
```

É o que o aviso existe para dizer: a partilha não se vê no `bind` de nenhuma das duas apps. Aqui é de propósito.

```sh
$ jk explain esp_io --env sim
```

```
esp_io · local/bench_io@1 · instância composta (joker.toml:82)
  status        led_esp · std/led@1 · composta · Ok
  boot          botao · std/button@1 · composta · Ok
  knob          toque · std/analog_in@1 · composta · Ok
  rssi          in i64      esp.rssi · -55 good
  uptime        in f64 s    esp.uptime · 1 204 s good
  online        consulta    good(uptime) · true
  saúde         Ok
  caminhos      esp_io.boot.clicked ≡ botao.clicked · esp_io.status.on ≡ led_esp.on ≡ esp.led
  profundidade  2: bench_io → led, button, analog_in (JC0501: máximo 3)
```

No `jk dev --env sim --panel`, carrega no botão do ESP32 e vê o `app.bench_diag.cliques` a subir. O LED não muda, porque a `luz` manda nele. Para a `luz` (`jk app stop luz --node local`, noutro terminal, com o `jk dev` a correr): o lease dela expira, o nível `control:14` passa a ser o ativo, e o LED do ESP32 segue o toque. Com `jk app start luz --node local`, a `luz` volta a ganhar. Os cenários continuam a passar: com o ESP32 em falha, a `bench_diag` também não propõe (`online` é falso), como a `luz` 0.4.1.

Quando acabares, volta ao ramo principal. A parte 9 parte do commit da secção 11:

```sh
$ git switch main
```

**Estado no fim desta parte:** app `luz` **0.6** e app `status_light_ctl` **1.0.0**. Ficheiros novos: `classes/local/status_light.jkc`, `classes/local/status_light.conformance.toml`, `apps/status_light_ctl/`. Alterados: `joker.toml` (`[instances.sinal]`, `[outputs."sinal.mode"]`, `[outputs."sinal.blink_period"]`, `[apps.status_light_ctl]`, `bind` da `luz`, `order` do `main`), `apps/luz/`, `joker.sim.lock`, `joker.lab.lock`, `joker.zero.lock`, e a última expectativa do `sim/scenarios/pisca.scenario.toml`. O `sim/scenarios/falha_esp.scenario.toml` não mudou. O lab está na geração 9; o zero, parado, ficou na 0.5. O extra da secção 12 (a classe `local/bench_io@1`, a instância `esp_io` e a app `bench_diag`) fica no ramo `extra-bench-io`, e não entra neste estado.

**Próximo:** [Parte 9 — O ESP32 passa a Joker Micro: o Joker Link e o `safe` no MCU](09-joker-micro.md).
