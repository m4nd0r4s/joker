# 01 — BESS com FV: peak shaving e redução por ordem do operador de rede

O projeto completo da rodada 1 do documento 08: um template partilhado, o manifesto de um site, as duas apps em Rust, os cenários de teste e o procedimento de comissionamento. É o exemplo de referência para projetos de energia.

A previsão de carga (`[models.load_forecast]`, a ligação `model:` e o `Tensor` do plano) é **1.1** (ADR 0019): está marcada "(1.1)" nos blocos. Sem ela, o resto corre numa runtime 1.0, com a porta `forecast` declarada `optional`.

---

## 1. Estrutura

```
bess/
  templates/joker.bess-v2.toml    overlay partilhado por todos os sites BESS
  sites/lisboa-01/joker.toml      só o que é deste site
  sites/lisboa-01/joker.lock
  sites/lisboa-01/joker.sim.toml  o mesmo site com os dispositivos em driver = "sim" (§6)
  apps/peak-shaver/               app de peak shaving + plano
  apps/rse-curtail/               app de redução por telecomando
  models/load-tcn-v3.jkm
  sim/*.sim.toml                  planta: carga, FV, bateria, medidor
  sim/scenarios/*.scenario.toml   cenários de teste (§6)
```

## 2. O template

```toml
# templates/joker.bess-v2.toml
[project]
joker = "1.1"                        # usa [models] (1.1, ADR 0019)
mode  = "prod"

[target]
kernel = "preempt_rt"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
ml = { cpus = [1], memory = "512 MiB" }
bg = { cpus = [0], memory = "512 MiB" }

[cycles.fast]
period = "200ms"
resources = "rt"
trace = { continuous = "24h" }

[outputs."pcs.p_set"]
rate  = "25 kW/s"
safe  = "0 kW"
safer = "equal"
interlock = "meter.f > 49.8 Hz && meter.f < 50.2 Hz && bms.soc >= 5 % && bms.soc <= 98 % && !alarm.bms_fault.active"
requires_good = ["meter.p_total", "bms.soc"]
grace = "1s"
verify = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }
max_lease = "15m"

[outputs."inv*.w_max_lim_pct"]
range = ["0 %", "100 %"]
safe  = "0 %"
safer = "lower"
max_lease = "10s"

[alarms.bms_fault]
when = "bms.state == BmsState.fault"
severity = "critical"
actions = { trip = ["pcs.p_set"] }

[alarms.grid_limit_exceeded]
when = "avg(meter.p_total, 1m) > calc.grid_limit"
delay_on = "1m"
severity = "high"

[apps.peak_shaver]
source = "oci://registry.example.com/joker/peak-shaver:1.4.2"
writer = { level = "optimizer", priority = 13, lease = "2s" }
cycles = { control = "fast" }

[apps.rse_curtail]
source = "oci://registry.example.com/joker/rse-curtail:1.0.0"
writer = { level = "supervisor", priority = 7, lease = "2s" }
cycles = { track = "fast" }

# (1.1) modelos de ML: ADR 0019, ref/01 §6.1
[models.load_forecast]
source = "oci://registry.example.com/joker/models/load-tcn:3"
backend = ["burn:cpu"]
resources = "ml"
deadline = "5s"

[telemetry.record]
"meter.**" = { every = "1s", keep = { raw = "14d", "1m" = "730d" } }
"pcs.**"   = { every = "1s", keep = { raw = "14d", "1m" = "730d" } }
"inv*.**"  = { every = "5s", keep = { raw = "14d", "15m" = "730d" } }
"rse.**"   = { on_change = true, keep = { raw = "730d" } }        # prova de cumprimento

[telemetry.uplink.cloud]
kind = "mqtt-sparkplug"
broker = "mqtts://mqtt.example.com:8883"
group_id = "bess"
budget = "500 MB/month"
classes = { alarms = "alarm.**", high = ["meter.p_total", "pcs.p_ac", "bms.soc", "rse.**"], low = "**" }
```

## 3. O site

```toml
# sites/lisboa-01/joker.toml
extends = ["../../templates/joker.bess-v2.toml"]

[project]
name = "lisboa-01"

[target]
board = "revpi-connect-5"

[devices.meter]
driver = "modbus-tcp"
address = "192.168.10.20:502"
profile = "janitza/umg604@2"
poll = "200ms"

[devices.pcs]
driver = "modbus-tcp"
address = "192.168.10.30:502"
profile = "acme/pcs-125@3"
poll = "200ms"
align = "meter"
fallback = { configure = true }

[devices.bms]
driver = "modbus-tcp"
address = "192.168.10.31:502"
profile = "acme/bms-250@1"
poll = "1s"

[devices.inv1]
driver = "modbus-tcp"
address = "192.168.10.41:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.inv2]
driver = "modbus-tcp"
address = "192.168.10.42:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.inv3]
driver = "modbus-tcp"
address = "192.168.10.43:502"
profile = "sunspec"
discover = true
poll = "500ms"
fallback = { configure = true }

[devices.io]
driver = "revpi"

[devices.io.signals]
di_1 = { alias = "rse.k1" }
di_2 = { alias = "rse.k2" }
di_3 = { alias = "rse.k3" }
di_4 = { alias = "rse.k4" }

[calc]
grid_limit = "400 kW"
rse_limit = "if rse.k4 then 0 % else if rse.k3 then 30 % else if rse.k2 then 60 % else 100 %"

[outputs."pcs.p_set"]
range = ["-125 kW", "125 kW"]

[apps.peak_shaver.bind]
grid_power = "meter.p_total"
soc        = { signal = "bms.soc", as = "fraction" }
battery    = "pcs.p_set"
forecast   = "model:load_forecast"   # (1.1); numa runtime 1.0, tirar e declarar a porta `optional`

[apps.peak_shaver.params]
limit   = "400 kW"
reserve = "10 %"

[apps.rse_curtail.bind]
limit    = "calc.rse_limit"
inverters = "inv*.w_max_lim_pct"
```

## 4. A app `rse-curtail`

Pequena de propósito: a política de distribuição fica aqui, e não no núcleo (documento 08, 1.4d).

```rust
use joker::prelude::*;

#[joker::app(name = "rse_curtail", version = "1.0.0")]
pub struct Curtail {
    #[input(unit = "%")]
    limit: Input<f64>,

    /// Uma porta "fan-out": liga-se a um seletor (inv*.w_max_lim_pct) e dá um array.
    #[output(unit = "%", many)]
    inverters: OutputMany<f64>,
}

#[joker::cycle(period = "200ms", wcet = "5us")]
fn track(io: &mut CycleIo<Curtail>, _st: &mut ()) -> Result<()> {
    // Sem leitura boa do telecomando → não escrever. O lease expira e os inversores
    // vão para o safe (0 %). Um contacto partido é tratado como a ordem mais restritiva.
    let limit = io.limit.get()?;
    for inv in io.inverters.iter_mut() {
        inv.set(limit);            // distribuição igual; outras políticas substituem esta linha
    }
    Ok(())
}
```

## 5. A app `peak-shaver`

A declaração e o ciclo estão no documento 04, §3.1–3.3. O que falta é o plano. O `from_forecast` usa `Tensor`, que é da 1.1 (ref/04 §12), como o modelo que o alimenta:

```rust
#[derive(Record, Clone, Copy, Default)]
pub struct Plan {
    target_soc: f64,     // fração
    limit_kw: f64,       // limite efetivo para o próximo período
    valid_until: UtcSeconds,
}

impl Plan {
    /// Sem previsão fiável: segura a bateria para o pior caso.
    fn conservative(now: Instant) -> Self {
        Plan { target_soc: 0.8, limit_kw: f64::INFINITY, valid_until: now.utc_seconds() + 30 * 60 }
    }

    /// Com previsão: reserva energia suficiente para os picos das próximas 24 h.
    fn from_forecast(f: &Tensor, limit: f64, now: Instant) -> Self {
        let load: &[f32] = f.as_slice();                  // 96 passos de 15 min, em kW
        let excess_kwh: f64 = load.iter().map(|&p| (p as f64 - limit).max(0.0) * 0.25).sum();
        let capacity_kwh = 250.0;
        let target = (excess_kwh / capacity_kwh * 1.2 + 0.1).clamp(0.2, 0.95);
        Plan { target_soc: target, limit_kw: limit, valid_until: now.utc_seconds() + 30 * 60 }
    }
}
```

## 6. Cenários de teste

```toml
# sim/scenarios/rse-zero.scenario.toml
schema = 1

[scenario]
name = "ordem de 0 % cumprida em ≤ 1 tick após a leitura"
manifest = "sites/lisboa-01/joker.sim.toml"     # overlay do site com os dispositivos em driver = "sim" (ref/03 §2.5.3)
duration = "10m"

[[set]]
at = "3m"
signal = "rse.k4"
value = true

[[expect]]
at = "3m+400ms"                    # ≤ 1 poll do DI + 1 tick
signal = "inv*.w_max_lim_pct"
equals = "0 %"

[[expect]]
during = "3m+400ms .. 10m"
signal = "inv*.ac.p"
max = "1 kW"
```

```toml
# sim/scenarios/frequencia.scenario.toml
schema = 1

[scenario]
name = "sub-frequência → PCS a 0 kW"
manifest = "sites/lisboa-01/joker.sim.toml"     # overlay do site com os dispositivos em driver = "sim" (ref/03 §2.5.3)
duration = "2m"

[[set]]
at = "30s"
signal = "meter.f"
value = "49.7 Hz"

[[expect]]
at = "30s+200ms"
signal = "pcs.p_set"
equals = "0 kW"
outcome = "rejected(interlock)"
```

## 7. Comissionamento

```sh
jk doctor --node lisboa-01 --rt            # kernel, isolcpus, watchdog, relógio
jk discover sunspec://192.168.10.41 --node lisboa-01
jk profile test acme/pcs-125@3 --node lisboa-01 --uri modbus-tcp://192.168.10.30
jk lock --node lisboa-01                   # mapas SunSpec, firmware, fallbacks (M1, M2)
jk bench --node lisboa-01
jk check
jk apply --node lisboa-01 --confirm 10m
# testes funcionais: forçar os DI do telecomando, confirmar a resposta dos inversores
jk decisions --output 'inv*.w_max_lim_pct' --since 10m --node lisboa-01
jk confirm --node lisboa-01
```

Um relatório de cumprimento para o operador de rede sai do registo de decisões (M3):

```sh
jk decisions --output 'inv*.w_max_lim_pct' --cause rse.k3 --from 2026-09-01 --to 2026-09-15 --report pdf
```
