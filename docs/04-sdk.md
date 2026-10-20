# 04 — SDK de apps

Uma app é um **componente WebAssembly** que declara portas e exporta tarefas. Pode ser escrita em Rust (tier 1), TypeScript ou Python (tier 2). Este documento explica o contrato WIT, o SDK Rust, os SDKs de tier 2, os testes e a simulação. O detalhe normativo está nas referências (ADR 0023): o mundo WIT e o SDK Rust na ref/04, os SDKs de TypeScript e Python na ref/05, a simulação na ref/03 §2.4.

---

## 1. Princípios de DX

1. **O código é o manifesto da app.** As portas, as unidades, as gamas de parâmetros, as tarefas e os alarmes são declarados no código. O SDK emite-os numa secção custom `joker:meta` do componente, e não há um `app.toml` separado que possa ficar dessincronizado.
2. **Os erros aparecem o mais cedo possível:** tipo e unidade na compilação da app, ligação no `jk check`, e só a qualidade em runtime.
3. **A qualidade é explícita.** `get()` devolve `Result`, e o `?` é o caminho feliz.
4. **O ciclo e o async são tipos diferentes.** Não se consegue chamar `history` nem `invoke` numa tarefa de ciclo, porque o compilador não deixa.
5. **Tudo é testável sem hardware**, com relógio virtual e determinismo total.

---

## 2. O contrato WIT (`wit/joker-runtime.wit`)

O mundo das apps é o pacote `joker:runtime` (`@0.1.0` até à 1.0, `@1.0.0` na 1.0), no ficheiro `wit/joker-runtime.wit`. A referência normativa é a **ref/04 §2** (interfaces, funções e erros) e **§3** (a ABI de ciclo, byte a byte). Em resumo:

- **`types`:** qualidade (os 20 motivos, pela ordem dos códigos da ref/00 §4.3), `value`, `sample`, `instant`, `write-outcome` (com `clamped(clamp)`, que distingue gama e ritmo) e `record error { code, message }`, com um código do registo.
- **`cycle-io`** (instância de ciclo): leitura do latch, propostas, métodos de classe, publicados, eventos, caixas de correio, `now`, `dt-ns` e `random-u64`. Síncronas e sem alocação no host.
- **`async-io`** (instância async): leitura do slot, `snapshot`, `set` e `write` (o `write` resolve com o resultado final, depois do commit do grupo dono), métodos com resultado, ações (também longas, com progresso e cancelamento), `subscribe`, eventos, `history` e `sleep-until`.
- **`kv`** e **`secrets`** (só async), e **`log`** (as duas; autónoma, para que o `joker:driver` a importe com a mesma versão).
- **`guest`** (exportada): `init`, `on-params`, `on-cycle`, `run-task`, `on-action`, `export-state`, `migrate`. O caminho rápido é **uma chamada por tarefa e por tick**: a imagem de entradas empacotada (24 B por amostra) entra, o buffer de propostas sai. `on-cycle` devolve um `cycle-result` com as propostas **e** o erro, para que um `Err` não perca as propostas já feitas.
- **Reservadas para a 1.1** com `@unstable`: `ml`, `frames`, `blocks`, `write-at` (M6) e `propose` (M18). Não fazem parte do mundo estável, e um componente que as importe é recusado em `mode = "prod"` (`J-E0003`).

**Duas instâncias do mesmo componente** (documento 01, §5): na instância de ciclo, as importações de `async-io`, `kv` e `secrets` (e as reservadas `ml`, `frames`, `blocks`) fazem trap com `J-E0901 async API called from cycle`; na instância async, `cycle-io` faz trap com `J-E0902`. O SDK Rust impede estes erros em compilação (`J-E0701`), e os de tier 2 no `jk build` (`J-E0859`) e, se escaparem, em runtime com uma mensagem clara. O host fornece o WASI sempre virtualizado: relógios do latch no ciclo, aleatoriedade determinista, sem ficheiros, rede só em async e só para `net.allow` (ref/04 §2.7).

**Metadados** (secção `joker:meta`, CBOR determinístico, esquema em `spec/cddl/joker-meta.cddl`, ref/04 §5): nome e versão da app, mundo WIT (`joker:runtime@0.1.0`), linguagem, tier e versão do SDK, versão da ABI de ciclo, portas (nome, direção, tipo, unidade, grandeza, gama, `optional`; a posição de uma porta na tabela é o seu handle), tarefas (nome, tipo, gatilho, período pedido, WCET declarado), alarmes, esquemas de registos, **equipamentos** (nome, classe e major, `requires`, limites de `Many` e a lista de membros **que o handle conhece**), **classes implementadas** (`#[implements]`, com o hash do contrato), segredos (só os nomes), funcionalidades instáveis e a memória sugerida. `jk inspect app.wasm` mostra-os.

**Versões (M21):** até à 1.0, o `jokerd` serve o mundo atual e o minor anterior, através de um adaptador gerado; a partir da 1.0, serve todos os `joker:runtime@1.y` com y até ao seu (ADR 0022). Um mundo mais novo do que a runtime é recusado antes do deploy (`J-E0001`). Os enums só crescem no fim, e cada caso novo tem uma projeção declarada para os mundos anteriores (ref/04 §2.8).

---

## 3. SDK Rust

**Dependência.** O crate publicado chama-se `joker-sdk` (o nome `joker` está ocupado no crates.io), e o `Cargo.toml` gerado pelo `jk app new` importa-o como `joker`: `joker = { package = "joker-sdk", version = "0.1" }`. Daí o `use joker::prelude::*`.

### 3.1 Declarar uma app

```rust
use joker::prelude::*;

#[joker::app(name = "peak_shaver", version = "1.4.2")]
pub struct PeakShaver {
    #[input(unit = "kW", quantity = "active_power")]
    grid_power: Input<f64>,                      // o nome que o bind do documento 03, §8, liga

    #[input(unit = "1")]                         // fração 0..1
    soc: Input<f64>,

    #[output(unit = "kW", quantity = "active_power")]
    battery: Output<f64>,

    #[param(unit = "kW", range = "0..5000", default = 400.0)]
    limit: Param<f64>,

    #[param(unit = "%", range = "0..50", default = 10.0)]
    reserve: Param<f64>,

    #[internal]
    plan: Internal<Plan>,                        // escrito pelo async, lido pelo ciclo

    #[model(inputs = "f32[1,96,4]", outputs = "f32[1,96]")]   // 1.1: feature `unstable-ml`
    forecast: Model,

    #[alarm(severity = "medium", message = "plano expirado")]
    plan_stale: Alarm,

    #[output(event)]
    low_soc: EventOutput<LowSoc>,
}

#[derive(Record, Clone, Copy, Default)]          // ≤ 64 bytes, só escalares
pub struct Plan {
    target_soc: f64,
    limit_kw: f64,
    valid_until: UtcSeconds,
}

#[derive(joker::Event)]
pub struct LowSoc {
    soc: f64,
}
```

A porta `#[model]` e o `infer` são da 1.1 (reservados com `@unstable`, ref/04 §4.15): sem a feature, `J-E0718`, e um componente que os use é recusado em produção. A mesma app na v1.0, sem modelo e com o plano calculado a partir do historian, está na ref/05 §20.1, lado a lado em Rust, TypeScript e Python.

**Portas múltiplas.** `#[output(many)]` com `OutputMany<T>` (e `InputMany<T>`) liga-se a um seletor do manifesto (`"inv*.w_max_lim_pct"`) e dá um array ordenado por caminho. O número de elementos é fixado no `init`.

**Sinais publicados.** `#[publish]` declara sinais derivados da app (`Published<T>`, ou `Counter` para contadores monotónicos), visíveis como `app.<app>.<nome>`, gravados e usáveis em alarmes e expressões como qualquer outro sinal.

O `#[joker::app]` gera duas vistas, `CycleIo<PeakShaver>` e `TaskIo<PeakShaver>`, com os métodos certos em cada uma, e a secção `joker:meta`.

### 3.2 Tarefas de ciclo

```rust
#[derive(Default)]
struct Ctl {
    integral: f64,
}

#[joker::cycle(period = "200ms", wcet = "40us")]
fn control(io: &mut CycleIo<PeakShaver>, st: &mut Ctl) -> Result<()> {
    let grid = io.grid_power.get()?;             // Err(Quality) → o tick desta tarefa termina aqui
    let soc  = io.soc.get()?;
    let plan = io.plan.get_or_default();         // antes do primeiro replan: valid_until = 0

    let stale = plan.valid_until < io.now().utc_seconds();
    io.plan_stale.set(stale);                    // raise()/clear() numa só chamada

    // Sem plano válido, vale o parâmetro. Com o Plan::default(), o limite seria 0 kW
    // e a bateria descarregaria para cobrir toda a importação da rede.
    let limit = if stale { io.limit.get() } else { plan.limit_kw.min(io.limit.get()) };
    let excess = grid - limit;
    st.integral = (st.integral + excess * io.dt().as_secs_f64()).clamp(-50.0, 50.0);

    let floor = io.reserve.get() / 100.0;                // % → fração, como o soc
    let sp = if soc <= floor { 0.0 } else { (excess + 0.1 * st.integral).max(0.0) };
    io.battery.set(sp);                          // proposta; o envelope decide
    Ok(())
}
```

- **O estado local** (`st`) vive na memória da instância de ciclo e é inicializado com `Default`. Sobrevive entre ticks e, se a app implementar `#[joker::state(persist)]`, também ao hot-swap (via `export-state`/`migrate`).
- **Não há `async`, alocação nem I/O no ciclo.** O `init` pode alocar (estado inicial, tabelas); durante o `on-cycle`, o alocador global da instância de ciclo faz trap (`J-E0903`), e o harness mostra o tick e a pilha. A API de ciclo só devolve tipos `Copy` ou limitados (`BoundedVec`, `BoundedString`), e o estado usa-os também. (Um alocador bump reposto a cada tick deixaria um `Vec` guardado no `st` a apontar para memória reutilizada no tick seguinte, sem erro nenhum: ref/04 §4.13.)
- **`period` é um pedido**, não uma ordem: o manifesto escolhe o grupo. Se o período do grupo divergir mais do que `period_tolerance` (por omissão ±50%), aparece `J0130`. Por isso as tarefas usam `io.dt()` e não constantes.
- Um erro devolvido (`Err`) termina a tarefa neste tick sem descartar as escritas já propostas, e fica registado (o `cycle-result` leva as duas coisas). Um **panic** é trap: as propostas do tick são descartadas (documento 01, §9).

### 3.3 Tarefas assíncronas

```rust
#[joker::task(every = "15m", jitter = "30s")]
async fn replan(io: &TaskIo<PeakShaver>) -> Result<()> {
    let now = io.now();
    let hist = io.grid_power.history(now - 24.h(), now, 15.mins()).await?;   // 96 pontos
    let x = Tensor::from_features(&hist, &calendar_features(now))?;         // função da app; 1.1
    let y = io.forecast.infer([x]).await?;                                  // 1.1

    if y.quality.is_uncertain() {
        log::warn!("forecast OOD; a usar o plano conservador");
        io.plan.set(Plan::conservative(now));
        return Ok(());
    }

    io.plan.set(Plan::from_forecast(&y.outputs[0], io.limit.get(), now));
    Ok(())
}

#[joker::task(on = "soc")]                                           // corre a cada mudança de `soc`
async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()> {
    if let Some(&v) = s.good() {                                     // só amostras good
        if v < 0.05 {
            io.low_soc.emit(LowSoc { soc: v });
        }
    }
    Ok(())
}
```

Gatilhos: `every = "…"`, `on = "porta"` (mudança), `on_event = "porta"`, `at = "cron: 0 */15 * * * *"`, e `startup` (ref/04 §4.3.2). O minuto é `mins()`, e não `min()`, que colidiria com `Ord::min`. Um `s.value` lido sem ver a qualidade trataria um `bad(comm_failure)` como um SOC válido.

### 3.4 Partilha entre ciclo e async

| Mecanismo | Direção | Semântica |
|---|---|---|
| `Internal<T>` | um só escritor, declarado: `#[internal(writer = "async")]` (omissão) ou `"cycle"` | último valor (slot largo ≤ 64 B), seqlock; o ciclo lê do latch |
| `Mailbox<T, N>` | `#[mailbox(to = "cycle")]` ou `to = "async"` | fila SPSC limitada (N ≤ 64, mensagens ≤ 256 B); `try_send` nunca bloqueia; as perdas são contadas |
| eventos (`emit`) | qualquer → qualquer consumidor | fila por consumidor, gravado se declarado |

Não há `static mut` partilhado entre instâncias, porque não existe: são memórias lineares diferentes, e um `static` mutável existe duas vezes, uma em cada instância. O SDK não o consegue detetar (um `static` não se distingue de outro), e por isso a regra está na documentação e nos testes: o harness corre sempre as duas instâncias (ref/04 §6).

### 3.5 Ações, streams e escritas agendadas

```rust
#[derive(joker::Enum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class { Ok = 0, Defect = 1 }

#[derive(joker::Record)]
pub struct Detection { class: Class, score: f64 }

#[derive(joker::Event)]
pub struct Detections { frame_ts_hw: MonoNanos, items: BoundedVec<Detection, 64> }

#[joker::app]
pub struct Inspector {
    #[input(event)]                   detections: EventInput<Detections>,   // o tipo vem do genérico
    #[input]                          photoeye: Input<bool>,
    #[output]                         ejector: Output<bool>,
    #[action]                         reset_line: Action<(), ()>,
    #[param(unit = "ms", default = 400.0)] travel: Param<f64>,
}

#[joker::task(on_event = "detections")]
async fn decide(io: &TaskIo<Inspector>, d: Detections) -> Result<()> {
    let bad = d.items.iter().any(|b| b.class == Class::Defect && b.score > 0.6);
    let t_eject = d.frame_ts_hw + io.travel.get().ms();             // ts monotónico do flanco do trigger
    if bad {
        io.ejector.write_at(true, t_eject).await?;                   // 1.1; o manifesto define pulse e on_late
    }
    Ok(())
}
```

As escritas agendadas (`write_at`, M6) são da 1.1, como os streams e a ML: reservadas com `@unstable` e a feature `unstable-scheduled-writes` (D5 do documento 22; ref/04 §2.9). O exemplo mostra a forma prevista.

### 3.6 Handles de classe (M24)

Além das portas, uma app pode declarar **equipamentos** tipados, gerados a partir das classes do registo ou definidos na própria app:

```rust
use joker::classes::std::{Led, Button, Pump};

#[joker::app(name = "demo", version = "0.1.0")]
pub struct Demo {
    #[device] led: Led,                                 // led.on(), led.off(), led.toggle()
    #[device] botao: Button,                            // botao.clicked()? (consulta do núcleo)
    #[device(requires = ["pressure"])] pumps: Many<Pump, 1, 8>,   // entre 1 e 8 instâncias
}
```

`Many<C, MIN, MAX>` leva dois parâmetros constantes: a forma `Many<Pump, 1..=8>` não é Rust válido, e o parser falharia antes de a macro a ver (ref/04 §4.8.3). Um `Many` dá `iter()`, `len()`, `healthy()`, `get(nome) -> Option` e `by_name(nome) -> Result`.

Uma app pode também implementar uma classe, com `#[joker::implements(Pump)]`, e oferecer a outras apps um equipamento virtual. Tudo isto compila para portas: o documento 14 tem a definição completa, os casos grandes e os ataques ao desenho.

### 3.7 Testes unitários (`joker-test`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn nunca_descarrega_abaixo_da_reserva() {
        let mut h = Harness::<PeakShaver>::new()
            .group("control", 200.ms())
            .param("limit", 400.0)
            .param("reserve", 10.0)
            .input("grid_power", Series::ramp(300.0, 700.0, 10.mins()))
            .input("soc", Series::linear(0.30, 0.05, 10.mins()))
            .internal("plan", Plan { target_soc: 0.5, limit_kw: 400.0, valid_until: UtcSeconds::MAX });

        h.run_for(10.mins());                                         // relógio virtual: corre em ms

        let sp = h.output("battery");
        let soc = h.input_series("soc");
        assert!(sp.when(soc.lt(0.1)).all(|v| v == 0.0));
        assert!(h.traps().is_empty());
    }

    #[test]
    fn qualidade_ma_nao_escreve() {
        let mut h = Harness::<PeakShaver>::new()
            .input("grid_power", Series::constant(500.0).with_quality_at(30.s(), Quality::bad(CommFailure)))
            .input("soc", 0.5);
        h.run_for(60.s());
        assert!(h.output("battery").after(30.s()).no_new_writes());
    }
}
```

O `Harness` corre o componente real (`.wasm`) no wasmtime com o relógio virtual, e não uma versão nativa: o que se testa é o que vai para produção.

---

## 4. SDKs de tier 2

Os SDKs de TypeScript e Python usam o mesmo mundo, a mesma imagem empacotada e o mesmo `joker:meta` do Rust. A API completa, a paridade com o Rust e o empacotamento estão na **ref/05** (pacotes `@joker/sdk` e `joker_sdk`). Os nomes de portas, parâmetros e membros ficam em `snake_case` em todas as linguagens, também em TypeScript (ref/00 §2).

### 4.1 TypeScript (via ComponentizeJS / jco)

```ts
import { app, input, output, param, cycle, task, log } from "@joker/sdk";

export default app({
  name: "hvac_scheduler",
  version: "0.2.0",
  inputs:  { zone_temp: input.f64("°C"), occupied: input.bool() },
  outputs: { setpoint: output.f64("°C") },
  params:  { comfort: param.f64({ unit: "°C", default: 21.5, range: [16, 26] }) },

  cycles: {
    track: cycle("1s", (io) => {
      const occ = io.occupied.get();          // lança QualityError se não for good
      io.setpoint.set(occ ? io.comfort.get() : io.comfort.get() - 3);
    }),
  },

  tasks: {
    report: task.every("1h", async (io) => {
      const hist = await io.zone_temp.history({ last: "24h", step: "15m" });
      log.info("média diária", { avg: hist.mean() });
    }),
  },
});
```

### 4.2 Python (via componentize-py)

```python
from joker_sdk import App, Input, Output, Param, app, cycle, log, task

@app(name="tank_level", version="0.1.0")
class Tank(App):
    level  = Input(float, unit="m")
    pump   = Output(float, unit="%")
    target = Param(float, unit="m", default=1.5, range=(0, 3))

    @cycle("500ms")
    def control(self) -> None:
        err = self.level.get() - self.target.get()      # a bomba esvazia o tanque
        self.pump.set(max(0.0, min(100.0, err * 80.0)))

    @task.every("10m")
    async def report(self) -> None:
        h = await self.level.history(last="1h", step="1m")
        log.info("nível", mean=h.mean())                 # média das amostras good
```

O estado não vive em `self`, que é uma vista só de leitura (uma atribuição dá `J-E0803`): as duas instâncias não partilham memória, e o estado de ciclo passa por `state=`, `Internal` ou `Mailbox` (ref/05 §10).

### 4.3 Limites de tier 2

**O tier 2 também corre no ciclo**, com um período mínimo. As pausas de GC acontecem dentro do tick, o `jk bench` mede-as, e a admissão usa o **máximo** medido, e não o p99,9, porque uma cauda de GC fora do p99,9 dava deadlines falhados em campo (D15 do documento 22).

| | Rust | TypeScript | Python |
|---|---|---|---|
| Tamanho do componente | 0,1–2 MB | 8–12 MB (máximo 32 MB) | 15–30 MB (máximo 64 MB) |
| `memory.max` por omissão (ciclo / async) | 16 / 64 MiB | 64 / 64 MiB | 128 / 128 MiB |
| Período mínimo de ciclo | 1 ms | **100 ms** (regra: `J-E0854`) | **500 ms** (regra: `J-E0854`) |
| Admissão de tempo real | p99,9 medido | máximo medido | máximo medido |
| GC no ciclo | não | sim, dentro do tick | sim, dentro do tick |
| Tipos e unidades | na compilação | tipos no `tsc`; unidades no `jk build` | tipos no mypy/pyright; unidades no `jk build` |
| Joker Micro | sim (nativo) | não (`J0720`) | não (`J0720`) |

Os mínimos, os máximos de WCET e de tamanho e as arquiteturas são regras do `jk check`; os tamanhos e os tempos são alvos a medir quando cada SDK existir (TypeScript na Fase 3, Python na 1.2). A tabela completa está na ref/05 §19. Se o async do WASI 0.3 não estiver estável nos toolchains quando o SDK sair, a instância async de tier 2 usa uma variante bloqueante do mundo numa thread `app` (plano B, ref/05 §29 D9). A regra continua: **controlo rápido em Rust**, e TS ou Python para lógica lenta, integrações e protótipos.

---

## 5. Apps nativas (escape)

`kind = "native"`: crate Rust compilado **dentro** do `jokerd` através de uma feature, com a mesma API do SDK (`joker::prelude`), mas sem Wasm. Serve para ciclos de 1 kHz com cálculo pesado. Não tem hot-swap nem sandbox, e exige a feature `native-apps` no build da runtime e assinatura com a chave `runtime`. É uma porta de saída, não o caminho normal. O mesmo código compila para o backend nativo do SDK (ref/04 §4.1 e §4.14), que é também o do Joker Micro (contrato `NativeApp`, ref/08 §2.11.5).

---

## 6. Simulação

O driver `sim` fornece plantas para `jk dev`, testes de site e CI:

```toml
# sim/tank.sim.toml
schema = 1

[signals]
level     = { type = "f64", unit = "m", init = 1.0 }
pump      = { type = "f64", unit = "%", direction = "output" }
inflow    = { type = "f64", unit = "m3/h", gen = { kind = "sine", mean = 2.0, amp = 1.0, period = "10m" } }

[dynamics]
# nível' = (inflow - k·pump) / área ; integrado pelo sim a cada 10 ms de tempo virtual
level = "integrate((inflow - 0.05 m3/h/% * pump) / 3.2 m2, min = 0 m, max = 3 m)"

[faults]
"level" = [{ at = "5m", quality = "bad(comm_failure)", for = "20s" }]
```

- **Geradores:** `constant`, `step`, `ramp`, `sine`, `square`, `noise`, `walk`, `replay` (CSV ou Parquet).
- **Dinâmicas:** expressões com `integrate`, `lag(x, tau)` e `delay(x, t)`, suficientes para tanques, térmicas RC, SOC de bateria, rampa de inversor. Os argumentos nomeados (`min =`, `max =`) são uma extensão da linguagem de expressões que só vale em `[dynamics]` (ref/03 §2.4.6).
- **Plantas em Wasm:** para dinâmicas reais, um componente no mundo `joker:sim/plant` com `step(dt, inputs) -> outputs`, escrito em qualquer linguagem.
- **Falhas injetadas:** qualidade, atraso, valores congelados, desconexão, saltos de relógio.
- **Realismo de hardware:** `bounce` (ressalto de contactos), `noise` (ruído de ADC), latência por pedido. Os bugs que só aparecem com hardware real devem aparecer primeiro no simulador.
- **Painel interativo** (`jk dev --panel`): um sinal com `panel = { kind = "lamp" | "momentary" | "toggle" | "slider" | "gauge", key/keys = … }` aparece no terminal e pode ser mexido com o teclado. Um `momentary` é um impulso de `pulse` (150 ms por omissão) a cada pressão, porque um terminal não dá o soltar da tecla. `[faults] panel = { key = …, toggle = "bad(…)" }` liga e desliga uma falha; `panel` e `"*"` são nomes reservados em `[faults]`.

**Cenários** (`sim/scenarios/*.scenario.toml`): ações `[[set]]` (valor ou `series`), `[[press]]` (atalho para um botão momentâneo) e `[[inject]]` (qualidade durante um intervalo); expectativas `[[expect]]` com `at` ou `during` e um de `equals`, `max`/`min`, `toggles`, `toggles_every = "500ms ± 20ms"`, `pattern = "on 100ms, off 100ms, …"`, `outcome`, `quality`, ou `alarm` + `state`. Os literais canónicos são em minúsculas (`"bad(comm_failure)"`, `"clamped_rate"`); as formas da CLI (`Bad(CommFailure)`) são sinónimos exatos. A trilha de estudo (`docs/trilha/`) usa todos. O formato completo dos modelos, dos cenários e dos casos de conformidade está na ref/03 §2.4–2.6.

`jk test` corre os cenários de `sim/` contra o **manifesto completo**, com o núcleo real, o envelope real e relógio virtual. Testa o *site*, e não só a app: "com a frequência a 49,7 Hz, o PCS vai para 0 kW em menos de 1 tick".

---

## 7. Replay

```sh
jk record --for 2h --out incidente.jktrace          # no dispositivo
jk replay incidente.jktrace --app peak_shaver@1.4.3 # no portátil, com a versão corrigida
```

Um traço contém o snapshot de entradas de cada tick dos grupos gravados, os resultados das escritas, as saídas de modelos não deterministas (M22), os parâmetros e as gerações de configuração. O replay reexecuta as apps com o mesmo relógio e compara as propostas. `--diff` mostra onde a nova versão diverge.

**O estado de partida.** O `jk record` guarda no início do traço o estado de cada app (o blob do `export-state`, o mesmo da troca a quente; ref/04 §4.12) num segmento de estado, e o `jk replay` começa desse estado, e não do zero. Só as apps com estado persistente (`#[joker::state(persist)]`) o exportam; as outras começam do `init`. Os traços extraídos do anel contínuo (`jk trace-dump`) e os reconstruídos do historian (`jk trace-from-history`) não têm esse segmento: o replay começa do `init`, e os primeiros ticks podem divergir do gravado até o estado da app convergir. O formato está no documento 16, §7.
