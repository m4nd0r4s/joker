# 00 — Primeiro projeto: um tanque, uma bomba, zero hardware

Percurso guiado. Ao fim: uma app em Rust a controlar o nível de um tanque simulado, com envelope, alarme, testes e replay, pronta a correr num Raspberry Pi real trocando só o bloco de dispositivos do manifesto.

Pré-requisitos: Rust estável com o alvo `wasm32-wasip2`, e a `jk` instalada.

---

## 1. Criar

```sh
jk new tanque --template sim-tank
cd tanque
```

```
tanque/
  joker.toml              o manifesto
  apps/level/             Cargo.toml, src/lib.rs
  sim/tank.sim.toml       planta simulada (modelo de simulação)
  sim/scenarios/          cenários de teste (*.scenario.toml)
```

## 2. A planta simulada

```toml
# sim/tank.sim.toml
schema = 1

[signals]
level  = { type = "f64", unit = "m", init = 1.0 }
pump   = { type = "f64", unit = "%", direction = "output" }
inflow = { type = "f64", unit = "m3/h", gen = { kind = "sine", mean = 2.0, amp = 1.5, period = "20m" } }

[dynamics]
level = "integrate((inflow - 0.06 m3/h/% * pump) / 3.2 m2, min = 0 m, max = 3 m)"   # área 3,2 m²
```

## 3. O manifesto

```toml
# joker.toml
[project]
name = "tanque"
mode = "dev"

[devices.tank]
driver = "sim"
model  = "sim/tank.sim.toml"

[cycles.ctl]
period = "100ms"

[outputs."tank.pump"]
range = ["0 %", "100 %"]
rate  = "20 %/s"                   # não arrancar a bomba de 0 a 100 num tick
safe  = "0 %"
interlock = "tank.level > 0.2 m"   # nunca bombear com o tanque quase vazio
requires_good = ["tank.level"]

[alarms.high_level]
when      = "tank.level > 2.7 m"
delay_on  = "5s"
severity  = "high"

[apps.level]
path   = "apps/level"
writer = { level = "control", priority = 10, lease = "1s" }
bind   = { level = "tank.level", pump = "tank.pump" }
params = { target = "1.5 m", kp = 80.0 }
cycles = { control = "ctl" }
```

## 4. A app

```rust
// apps/level/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "level", version = "0.1.0")]
pub struct Level {
    #[input(unit = "m")]
    level: Input<f64>,

    #[output(unit = "%")]
    pump: Output<f64>,

    #[param(unit = "m", range = "0..3", default = 1.5)]
    target: Param<f64>,

    #[param(unit = "1", range = "0..500", default = 80.0)]
    kp: Param<f64>,
}

#[joker::cycle(period = "100ms", wcet = "10us")]
fn control(io: &mut CycleIo<Level>, _st: &mut ()) -> Result<()> {
    // Nível alto → bombear mais. Um erro de qualidade termina o tick aqui,
    // não se escreve nada, o lease corre e, se continuar, a bomba vai para 0 %.
    let err = io.level.get()? - io.target.get();
    io.pump.set((err * io.kp.get()).clamp(0.0, 100.0));
    Ok(())
}
```

## 5. Correr

```sh
jk dev
```

```
joker 0.1.0 · dev · tanque · gen 1
  cycles   ctl 100ms  jitter p99 0.4ms (sem RT)   overruns 0
  devices  tank (sim)  up
  apps     level 0.1.0  running  wcet 3µs
  outputs  tank.pump = 34.2 % (Accepted, level=control:10)
```

Noutro terminal:

```sh
jk read 'tank.**' --watch
jk trace tank.pump          # do sim ao envelope, tick a tick
```

Gravar o ficheiro `lib.rs` recompila e troca a app a quente, sem perder ticks.

## 6. Ver o envelope a trabalhar

```sh
jk write tank.pump 100 --ttl 30s --reason "teste"
```

```
tank.pump: escrita manual (manual:4) durante 30 s
  envelope:  rate 20 %/s → a subir em rampa (≈ 3,3 s até 100 %)
  interlock: tank.level > 0.2 m  ✓ (1,43 m)
  prevalece sobre: control:10 (app level)
confirmar? [s/N] s
```

Quando o nível baixa de 0,2 m, o interlock corta a bomba mesmo com a escrita manual ativa. 30 s depois, o nível manual expira e a app volta a mandar.

## 7. Testar

```toml
# sim/scenarios/sensor-falha.scenario.toml
schema = 1

[scenario]
name = "sensor falha durante 20 s"
duration = "5m"

[[inject]]
at = "2m"
signal = "tank.level"
quality = "bad(comm_failure)"
for = "20s"

[[expect]]
during = "2m+1.2s .. 2m+20s"      # após o lease de 1 s e um tick
signal = "tank.pump"
equals = "0 %"

[[expect]]
at = "2m+25s"
signal = "tank.pump"
outcome = "accepted"
```

```sh
jk test
```

```
sim/scenarios/sensor-falha.scenario.toml ........ ok (5m simulados em 0,4 s)
apps/level (cargo test) ................ ok
```

## 8. Replay

```sh
jk record --for 10m --out sessao.jktrace
# mudar o kp na app, recompilar
jk replay sessao.jktrace --diff
```

O `--diff` mostra, tick a tick, onde as propostas da nova versão divergem das gravadas.

## 9. Para um Raspberry Pi real

Troca-se o dispositivo simulado por hardware (um sensor de nível 4–20 mA num ADC I²C e um variador por Modbus RTU). **A app não muda**, nem o envelope, o alarme ou a ligação: os caminhos `tank.level` e `tank.pump` passam a ser aliases dos sinais reais. Remove-se o `[devices.tank]` e acrescenta-se:

```toml
[target]
board = "rpi5"

[buses.rs485]
kind = "serial"
port = "/dev/ttyUSB0"
baud = 19200
parity = "even"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.adc]
driver  = "i2c"
bus     = "i2c1"
address = 0x48
profile = "ti/ads1115@1"
[devices.adc.signals]
# 4–20 mA sobre uma resistência de shunt de 150 Ω → 0,6–3,0 V na entrada A0
a0 = { alias = "tank.level", scale = { from = ["0.6 V", "3.0 V"], to = ["0 m", "3 m"] } }

[devices.vfd]
driver  = "modbus-rtu"
bus     = "rs485"
unit_id = 3
profile = "acme/vfd-mini@2"
[devices.vfd.signals]
speed_pct = { alias = "tank.pump" }
```

```sh
jk doctor --node pi.local
jk apply --node pi.local --confirm 5m
jk confirm --node pi.local
```
