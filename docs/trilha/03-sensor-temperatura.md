# Parte 3 — Um sensor real: a temperatura do CPU, alarmes, historian e a API

**Objetivo:** o LED `ACT` do Pi 3 pisca tanto mais depressa quanto mais quente está o CPU; acima de 70 °C dispara um alarme; a temperatura fica gravada, e lês tudo isto também sem a `jk`, pela API.

**Vais aprender:** uma dinâmica simulada · sinais `calc` com janelas · unidades de ponta a ponta (`°C` contra `K`) · qualidade, o `?` e o `safe` · alarmes (ciclo de vida, `ack`, `shelve`) · historian · a API HTTP e WebSocket · papéis · o cliente Python.

**Antes de começar:** a parte 2 feita. O nó `lab` está enrolado, o `joker.lab.toml` tem o `[devices.pi]` com o driver `sysfs`, e a app `luz` 0.1 pisca o `ACT`.

---

## 1. A temperatura no simulador

Não há sensores ligados ao Pi, mas há um que vem com ele: a temperatura do CPU, em `/sys/class/thermal/thermal_zone0/temp` (em m°C). Aquece com carga e arrefece sozinha, com uma constante de tempo de um a dois minutos: é uma planta de primeira ordem de verdade, e é ela que dá dinâmica à trilha sem comprar nada (documento 23, §3). Primeiro, no simulador:

```toml
# sim/pi.sim.toml
schema = 1

[model]
description = "Raspberry Pi 3: LED ACT e temperatura do CPU"

[signals]
act_led  = { type = "bool", direction = "output", panel = { label = "ACT", kind = "lamp" } }
carga    = { type = "f64", unit = "1", init = 0.0,
             panel = { label = "Carga", kind = "slider", keys = ["left", "right"], step = 0.1, min = 0.0, max = 1.0 } }
cpu_temp = { type = "f64", unit = "°C", init = "45 °C", panel = { label = "CPU", kind = "gauge" } }

[dynamics]
# Primeira ordem: tende para 45 °C em repouso e para 75 °C com carga total, com τ = 60 s.
cpu_temp = "lag(45 °C + 30 K * carga, 60s)"
```

- `carga` é um sinal que **só existe no simulador**: faz de `stress-ng`. Nenhuma app o lê.
- `30 K` e não `30 °C`: é uma **diferença** de temperatura. `°C + K` dá `°C`; `°C + °C` é `J0301` (documento 16, §4).
- `lag` tem estado, e por isso `init` é obrigatório (`J0833` sem ele, ref/03 §2.4.6).

Com `jk dev --env sim --panel`, o painel mostra o `ACT`, a `Carga` (setas ← →) e o `CPU`. Põe a carga a 1 e vê o CPU subir: 63 % do caminho num minuto, quase tudo em cinco.

## 2. Um `calc`: a média de 10 s

A temperatura crua tem degraus e ruído (o do Pi real tem). A app não deve reagir a cada leitura, e a decisão "média de quanto tempo?" é do site, não da app:

```toml
# joker.toml (acrescentar)
[calc]
temp_media = "avg(pi.cpu_temp, 10s)"
```

`calc.temp_media` é um sinal como os outros: tem valor, unidade (`°C`, inferida), qualidade e caminho. Lê-se com `jk read calc.temp_media` e grava-se no historian.

## 3. A app, versão 0.2

A regra da bíblia da trilha: o meio período desce linearmente de `half_period` (a `t_frio`) até 100 ms (a `t_quente`), e fica nesses extremos fora da gama.

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;

/// Meio período mínimo, a quente.
const RAPIDO_MS: f64 = 100.0;

#[joker::app(name = "luz", version = "0.2.0")]
pub struct Luz {
    #[output]
    led: Output<bool>,

    /// Temperatura do CPU (já filtrada pelo manifesto).
    #[input(unit = "°C")]
    temp: Input<f64>,

    #[param(unit = "ms", range = "20..5000", default = 500.0)]
    half_period: Param<f64>,
    #[param(unit = "°C", range = "0..100", default = 45.0)]
    t_frio: Param<f64>,
    #[param(unit = "°C", range = "0..110", default = 70.0)]
    t_quente: Param<f64>,
}

#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
}

#[joker::cycle(period = "20ms", wcet = "5us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // 1. A temperatura primeiro. Se não for Good, a tarefa termina aqui sem propor nada.
    let t = io.temp.get()?;

    // 2. x = 0 a frio, 1 a quente.
    let (frio, quente) = (io.t_frio.get(), io.t_quente.get());
    let x = ((t - frio) / (quente - frio)).clamp(0.0, 1.0);
    let lento = io.half_period.get();
    let meio = lento + x * (RAPIDO_MS - lento);

    // 3. O pisca da parte 1, com um meio período que muda.
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= meio {
        st.elapsed_ms -= meio;
        if st.elapsed_ms > meio {
            st.elapsed_ms = 0.0;          // o meio período encolheu de repente
        }
        st.on = !st.on;
    }
    io.led.set(st.on);
    Ok(())
}
```

```toml
# joker.toml
[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

A app não sabe que a temperatura vem de um ficheiro do kernel, nem que é uma média de 10 s. Sabe que tem uma porta `temp` em `°C`.

## 4. Unidades: o que o `jk check` diz

Troca, por engano, a unidade da porta para `K` (`#[input(unit = "K")]`) e corre `jk check --env sim`:

```
error[J0108]: grandeza incompatível na ligação
  --> joker.toml:24:39
   |
24 | bind   = { led = "pi.act_led", temp = "calc.temp_media" }
   |                                       ^^^^^^^^^^^^^^^^^ `calc.temp_media` é uma temperatura (°C);
   |                                                         a porta `temp` espera uma diferença de temperatura (K)
   = nota: °C é afim: um valor absoluto escreve-se em °C, uma diferença em K (documento 16, §4)
   = ajuda: declara a porta com `unit = "°C"`
```

`K` e `°C` têm a mesma dimensão, mas não a mesma **grandeza**: 70 K de diferença não é 70 °C. Com `unit = "%"` o erro seria `J0107` (unidade incompatível), porque aí nem a dimensão casa. O mesmo vale para os valores: `t_quente = "70 K"` nos `params` é `J0108`, e `when = "calc.temp_media > 70"` (sem unidade) é `J0301`. Repõe `°C`.

## 5. Qualidade e o `?`

`io.temp.get()?` devolve o valor só se a amostra for `Good` (documento 16, §3.1). Se não for, a tarefa devolve `Err` (`J-E0102`) **antes** de chamar `io.led.set`. Nesse tick não há proposta, o lease da app (3 × 20 ms = 60 ms) não é renovado, expira, e o árbitro aplica o `safe`: o LED apaga-se.

Porque é a escolha certa aqui: sem temperatura, a app não sabe a que ritmo piscar, e piscar a um ritmo inventado mentiria a quem olha. Um `Err` não é um trap: não conta para o crashloop, e a app volta a propor no primeiro tick em que a temperatura regressar (ref/04 §8, ponto 8).

**Como a qualidade chega à média.** `calc.temp_media` é uma janela: fica `Bad` quando as amostras `Bad` cobrem pelo menos 10 % da janela (1 s de 10 s), e volta a `Good` quando descem abaixo disso, ou seja, cerca de 9 s depois de o sensor recuperar (documento 16, §3.2). Vais ver os dois instantes no cenário do §7.

## 6. Um alarme

```toml
# joker.toml (acrescentar)
[alarms.cpu_quente]
when     = "calc.temp_media > 70 °C"
delay_on = "10s"
severity = "medium"
message  = "CPU a {calc.temp_media}"
```

O ciclo de vida é o da ISA-18.2 simplificada (documento 02, §11):

```
normal ──(condição 10 s)──▶ active_unacked ──ack──▶ active_acked
   ▲                              │                     │
   │                   condição falsa              condição falsa
   │                              ▼                     │
   └──────────ack─────────── rtn_unacked                │
   └────────────────────────────────────────────────────┘
```

Com `calc.temp_media` `Bad`, o alarme fica `unknown`: não se sabe se está quente. Com o `jk dev` a correr e a carga a 1, espera dois minutos:

```sh
$ jk alarms
```

```
id           severidade  estado           desde     mensagem
cpu_quente   medium      active_unacked   há 12 s   CPU a 70,9 °C
```

```sh
$ jk alarm ack cpu_quente
$ jk alarm shelve cpu_quente 30m "ensaio de carga na bancada"
```

```
cpu_quente: active_acked
cpu_quente: arquivado até 11:32 (30 min) · "ensaio de carga na bancada"
```

Arquivar não muda o estado do alarme: suprime a notificação durante o TTL, que é obrigatório e limitado por `max_shelve` (omissão 24 h). `jk alarm shelve cpu_quente 48h "…"` dá `J-E1202`. Um `ack` num alarme `normal` dá `J-E1204`: não há nada para reconhecer. Cada alarme também é um sinal: `jk read alarm.cpu_quente.state`.

## 7. O cenário `aquece.scenario.toml`

```toml
# sim/scenarios/aquece.scenario.toml
schema = 1

[scenario]
name     = "mais quente pisca mais depressa; alarme a 70 °C; sem temperatura, safe"
manifest = "joker.sim.toml"
duration = "460s"

[[expect]]
during        = "2s .. 9s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"      # 45 °C: meio período de 500 ms
[[set]]
at     = "10s"
signal = "pi.carga"
value  = 1.0
[[expect]]
at    = "125s"
alarm = "cpu_quente"
state = "normal"                    # a média passa os 70 °C por volta dos 123 s; faltam os 10 s de delay_on
[[expect]]
at    = "140s"
alarm = "cpu_quente"
state = "active_unacked"
[[ack]]
at    = "200s"
alarm = "cpu_quente"
[[expect]]
at    = "201s"
alarm = "cpu_quente"
state = "active_acked"
[[expect]]
during        = "320s .. 340s"
signal        = "pi.act_led"
toggles_every = "100ms ± 20ms"      # ~75 °C: acima de t_quente, meio período mínimo
[[set]]
at     = "360s"
signal = "pi.carga"
value  = 0.0
[[expect]]
at    = "390s"
alarm = "cpu_quente"
state = "normal"                    # arrefeceu, e já estava reconhecido
[[inject]]
at      = "420s"
signal  = "pi.cpu_temp"
quality = "bad(comm_failure)"
for     = "20s"
[[expect]]
at      = "430s"
signal  = "calc.temp_media"
quality = "bad"
[[expect]]
during = "422s .. 439s"
signal = "pi.act_led"
equals = false                      # sem temperatura: sem proposta, lease expirado, safe
[[expect]]
at    = "430s"
alarm = "cpu_quente"
state = "unknown"
[[expect]]
within = "450s .. 452s"
signal = "pi.act_led"
equals = true                       # a média voltou a Good por volta dos 449 s; o pisca regressa
```

```sh
$ jk test
```

```
apps/luz (cargo test) ........................ ok
sim/scenarios/pisca.scenario.toml ..................... ok
sim/scenarios/aquece.scenario.toml .................... ok (460 s simulados em 0,4 s)
```

O `pisca.scenario.toml` da parte 1 continua a passar sem alterações: em repouso o simulador está a 45 °C, e a 45 °C o meio período é o `half_period`. Os testes unitários da parte 1 precisam de uma linha a mais, `.input("temp", 45.0)`: sem ela a porta `temp` não tem amostra, o `get()?` falha, e o LED nunca acende. Os dois testes novos, com o `joker-test` (ref/04 §6):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use joker_test::prelude::*;

    #[test]
    fn quente_pisca_a_100ms() {
        let mut h = Harness::<Luz>::new().group("pisca", 20.ms()).input("temp", 72.0);
        h.run_for(5.s());
        assert!(h.output("led").after(1.s()).toggles_every(100.ms()));
    }

    #[test]
    fn sem_temperatura_apaga() {
        let temp = Series::constant(50.0).with_quality(2.s(), 3.s(), Quality::bad(CommFailure));
        let mut h = Harness::<Luz>::new().group("pisca", 20.ms()).input("temp", temp);
        h.run_for(4.s());
        assert!(h.output("led").between(2100.ms(), 3.s()).all(|v| v == false));
        assert!(!h.errors().is_empty());           // os Err do `?`, registados; nenhum trap
    }
}
```

## 8. No Pi 3

O `pi.cpu_temp` real vem do mesmo driver do `ACT`. No `joker.lab.toml`, acrescenta uma linha ao dispositivo:

```toml
# joker.lab.toml (acrescentar)
[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }          # m°C no ficheiro; °C no sinal (ref/01, anexo A)
```

E pede ao historian que grave a temperatura (um sinal que nenhuma entrada apanha não é gravado, ref/01 §5.17.1):

```toml
# joker.toml (acrescentar)
[telemetry.record]
"pi.cpu_temp"     = { every = "1s", keep = { raw = "7d", "1m" = "90d" } }
"calc.temp_media" = { every = "1s", keep = { raw = "7d" } }
```

```sh
$ jk check --env lab
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
$ jk read pi.cpu_temp calc.temp_media alarm.cpu_quente.state pi.act_led --watch --node lab
```

```
pi.cpu_temp              47,2 °C   good    idade 0,6 s
calc.temp_media          47,0 °C   good
alarm.cpu_quente.state   normal    good
pi.act_led               true      good    (control:12 · luz)
```

Agora aquece o CPU a sério, noutro terminal:

```sh
lab$ sudo apt install stress-ng
lab$ stress-ng --cpu 4 --timeout 5m
```

Em dois ou três minutos, o `ACT` passa a piscar depressa e, sem dissipador, a média passa os 70 °C e o alarme dispara (o Pi 3 B+ baixa o relógio a partir dos 60 °C, e por isso a subida abranda perto do fim). Quando o `stress-ng` acaba, a temperatura desce sozinha, e o alarme vai para `rtn_unacked`: a condição passou, mas ninguém o viu. Um `jk alarm ack cpu_quente --node lab` fecha-o.

O historian tem tudo:

```sh
$ jk history pi.cpu_temp --from -15m --step 10s --node lab
```

```
ts                     avg       min       max       count  quality
2026-09-25T10:41:00Z   47,3 °C   46,7 °C   47,8 °C   10     good
2026-09-25T10:41:10Z   55,9 °C   49,4 °C   60,1 °C   10     good
…
2026-09-25T10:44:30Z   72,6 °C   72,0 °C   73,1 °C   10     good
```

## 9. A API: a `jk` é só um cliente

Tudo o que a `jk` fez até aqui foi um pedido HTTP à API local do `jokerd` (documento 07, primeiro parágrafo; ref/09 §9.1). No Pi, o socket Unix está sempre ligado:

```sh
lab$ curl -sS --unix-socket /run/joker/api.sock 'http://joker/v1/values?sel=pi.cpu_temp'
```

```json
{"items": [{"path": "pi.cpu_temp", "value": 52.6, "unit": "°C", "quality": "good", "reason": "none",
            "ts_acq": "2026-09-25T10:52:07.412603118Z", "seq": 3811}]}
```

É a amostra da ref/00 §4.2: valor na unidade do sinal, **sempre com a qualidade**, carimbo de aquisição em UTC e o `seq`. O histórico é o mesmo, noutro endpoint (`GET /v1/history?sel=pi.cpu_temp&from=-15m&step=10s`), e os alarmes também (`GET /v1/alarms?state=active`).

**Subscrever.** Para ver mudanças sem perguntar, há o WebSocket `GET /v1/ws` com o subprotocolo `joker.v1` (ref/09 §6). Com o `websocat`:

```sh
lab$ websocat --protocol joker.v1 --ws-c-uri ws://joker/v1/ws ws-c:unix:/run/joker/api.sock
{"op":"subscribe","id":"1","topic":"signals","sel":["pi.cpu_temp","calc.temp_media"],"min_interval":"1s"}
```

```
{"op":"welcome","session":"01J8ZC…","role":"admin","heartbeat":"15s","resume_window":"60s",…}
{"op":"subscribed","id":"1","sub":"s1","topic":"signals","resolved":2,"min_interval":"1s"}
{"op":"resync","sub":"s1","seq":1,"items":[{"path":"pi.cpu_temp","value":52.6,…},{"path":"calc.temp_media",…}]}
{"op":"data","sub":"s1","seq":2,"items":[{"path":"pi.cpu_temp","value":52.9,"unit":"°C","quality":"good",…}]}
```

Os tópicos de estado são **amostrados**, não enfileirados: um cliente lento recebe menos vezes, e nunca trava o núcleo (ref/09 §6.5 e §6.6). O WebSocket só serve para ver: na 1.0 não há escritas por ele. Para scripts com `curl`, o mesmo motor está em SSE: `curl -N --unix-socket /run/joker/api.sock 'http://joker/v1/stream?topic=signals&sel=pi.cpu_temp'`.

**Do portátil, em Python.** O cliente `joker_client` resolve o nó pelas mesmas regras da `jk` (ref/05 §22.2): o contexto `lab` da parte 2 serve aos dois.

```python
# temperatura.py   (pip install "joker-client[pandas]")
from datetime import datetime, timedelta, timezone
from joker_client import Client

jk = Client.from_config("lab")
fim = datetime.now(timezone.utc)
h = jk.history(["pi.cpu_temp", "calc.temp_media"], start=fim - timedelta(hours=1), end=fim, step="10s")
df = h.to_pandas()                                   # colunas (caminho, campo)
temp = df.xs("value", axis=1, level="field")
ok = df.xs("quality", axis=1, level="field") == "good"
print(temp.where(ok).describe())                     # o que não é good fica NaN, à vista
```

A qualidade vem sempre com o valor, e não há opção para a deitar fora (ref/05 §26.1).

**Papéis.** Cada pedido tem um principal (o utilizador Unix no socket, o `CN` do certificado em TCP com mTLS) e um papel, ordenados (ref/09 §2.3):

| Papel | Pode | Exemplos |
|---|---|---|
| `viewer` | ler: valores, histórico, alarmes, decisões, subscrições | um painel, um notebook |
| `operator` | agir sobre o processo: escritas manuais, `ack`, `shelve`, ações, `refresh` | quem opera a bancada |
| `admin` | mudar o nó: gerações, `apply`, `confirm`, segredos, auditoria | quem faz deploy |

Em `mode = "dev"`, sem `[security.roles]`, qualquer principal autenticado é `admin` (ref/01 §5.20): é por isso que o `welcome` acima diz `"role":"admin"`. Em `prod`, `operator` e `admin` exigem os grupos `joker-ops` e `joker-admin` por omissão, e o resto fica `viewer`.

**Porque uma escrita pede TTL e motivo.** Força o `ACT` aceso pela API, sem TTL:

```sh
lab$ curl -sS --unix-socket /run/joker/api.sock -X POST http://joker/v1/outputs/pi.act_led/manual \
       -H Content-Type:application/json -d '{"value":true,"reason":"teste da API"}'
```

```json
{"type":"urn:joker:code:J-E1101","title":"escrita manual sem TTL (A10)","status":400,"code":"J-E1101",
 "help":"acrescenta \"ttl\" (1 s até max_manual, 8 h por omissão)"}
```

Com `"ttl":"30s"` passa, e é o mesmo que `jk write pi.act_led true --ttl 30s --reason "teste da API"`. O TTL **é** o lease da escrita manual: uma escrita esquecida não manda para sempre, e o pisca volta sozinho ao fim de 30 s (documento 16, A10). O motivo vai para o registo de decisões e para a auditoria, ligados pelo `request_id` (ref/09 §4): daqui a um mês, "quem acendeu isto e porquê" tem resposta. Nenhuma escrita pela API passa ao lado do árbitro e do envelope (ref/09 §2.6).

## 10. O que aconteceu por baixo

- **Sinais derivados.** `calc.temp_media` corre no grupo mais rápido que o consome (`main`, porque a app o lê), com o anel da janela pré-alocado (ref/01 §3.7; documento 02, §10).
- **Qualidade.** A regra das janelas e a do `get()` são as do documento 16, §3; o alarme sem entrada boa vai para `unknown` e não para `normal`.
- **O `?`** termina a tarefa sem propostas; o lease (16, A2 e A9) faz o resto. É a primeira camada do estado seguro da parte 2, agora disparada por uma entrada e não por uma app parada.
- **Alarmes** são sinais (`alarm.cpu_quente.state`, `.active`) e eventos gravados com `fsync` (ref/01 §5.15; ref/09 §5.8).
- **Historian:** só grava o que `[telemetry.record]` pede, por camadas (`raw`, `1m`) com retenção própria (documento 01, §12).
- **A API** é a única porta do nó; a `jk`, o `curl`, o `websocat` e o Python são clientes dela, com os mesmos papéis e a mesma auditoria (documento 01, §14; ref/09).

## 11. Exercícios

1. Acrescenta `delay_off = "30s"` ao alarme e repete o `stress-ng`. O que muda no fim do ensaio? (Se o teu Pi tiver dissipador e não passar dos 65 °C, baixa o limite para `"60 °C"` só para o ensaio, e repõe-no.)
2. Troca a média de 10 s por `"avg(pi.cpu_temp, 1m)"`. Que instantes do `aquece.scenario.toml` deixam de passar, e porquê? Corrige o cenário, e não a app.
3. Escreve um script Python que subscreve o tópico `alarms` (`jk.subscribe(alarms=True)`) e imprime cada transição do `cpu_quente`.
4. Com `jk decisions --output pi.act_led --since 5m --node lab`, encontra a escrita manual do §9 e o seu motivo.

```sh
$ git add -A && git commit -qm "trilha(03): temperatura do CPU, alarme cpu_quente, historian e API; luz 0.2"
```

**Estado no fim desta parte:** app `luz` 0.2.0, no lab na geração 3; `sim/pi.sim.toml` com `carga` e `cpu_temp`; `joker.toml` com `[calc] temp_media`, `[alarms.cpu_quente]` e `[telemetry.record]`; `joker.lab.toml` com `pi.cpu_temp`; cenário novo `sim/scenarios/aquece.scenario.toml`.

**Próximo:** [Parte 4 — o ESP32 como equipamento Modbus: escrever o perfil, o botão e o LED](04-esp32-modbus.md).
