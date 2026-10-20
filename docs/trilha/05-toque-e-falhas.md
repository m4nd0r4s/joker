# Parte 5 — O toque como ajuste, qualidade, falhas, fallback e replay

**Objetivo:** o sensor de toque do ESP32 passa a ser um "botão rotativo" que desloca o limite quente do pisca; depois, parte-se a comunicação com o ESP32 de propósito, no simulador e na bancada, e vê-se o sistema continuar a piscar pelo Pi, o alarme disparar, o equipamento cair para o seu fallback, e o traço gravado a reproduzir uma versão corrigida da app no portátil.

**Vais aprender:** entradas analógicas ruidosas · `deadband` e `stuck_detect` de aquisição · `calc` adimensional · `get_or` e a qualidade substituída no traço · três políticas de qualidade na mesma app · cenários com falhas (`[[inject]] disconnect`) · perda de comunicação real · `jk device stop` · fallback do equipamento (M1) e keepalive · `jk status`, `jk trace`, `jk decisions` · `jk record` e `jk replay --diff` · determinismo: o relógio é uma entrada.

**Antes de começar:** a parte 4 feita. A app `luz` está na 0.3, o perfil `local/bench-esp32@1` existe, o `sim/esp.sim.toml` simula o ESP32, e o `joker.lab.toml` fala com o ESP32 por Modbus RTU pela ponte USB. O ESP32 está ligado por micro-USB ao Pi 3, com o firmware de bancada.

---

## 1. O toque é um sinal analógico e ruidoso

O pino T0 (GPIO4) do ESP32 mede a capacidade do pino. Sem dedo, a leitura crua anda perto de 60; com o dedo em cima, desce para ~15. Nunca fica parada: varia com a humidade, a temperatura e o cabo. É o "potenciómetro" da bancada (documento 23, §3).

```sh
$ jk read esp.touch --watch --node lab
```

```
esp.touch   61   good   idade 40 ms    (modbus-rtu esp32 · in[0] · deadband 5)
esp.touch   58   good   idade 60 ms
esp.touch   17   good   idade 30 ms    ← dedo
```

O perfil da parte 4 já declara `deadband = 5` no `touch`: variações abaixo de 5 contagens não chegam a atualizar o sinal. É um filtro **de aquisição** (documento 01, §3.3): corre no núcleo, antes de qualquer app, e poupa eventos `changed`, o historian e o traço.

Os teus números podem ser outros: mede-os antes de continuar, sem dedo e com o dedo, e usa-os no §2.

**`stuck_detect`.** Um sinal que devia variar sempre e fica bit a bit igual durante muito tempo é suspeito: pode ser o firmware parado a responder sempre o mesmo registo. `stuck_detect = "10m"` marca-o `uncertain(frozen)` (documento 02, §4). Atenção à ordem da aquisição (ref/03 §3): `deadband` → `stuck_detect`. O `stuck_detect` vê o valor **depois** do deadband, e um toque em repouso pode ficar dentro das 5 contagens muito tempo. Por isso o tempo é longo (10 min, e não 30 s), e por isso a app trata `uncertain` como trata `bad`: um falso positivo só faz a app manter o último valor bom, que é o mesmo.

## 2. Do valor cru a um ajuste

A app não quer saber de contagens: quer um ajuste de 0 a 1. A conversão é do site, e fica no manifesto comum:

```toml
# joker.toml (comum) — o que muda face à parte 4
[signals."esp.touch"]
stuck_detect = "10m"

[calc]
temp_media  = "avg(pi.cpu_temp, 10s)"
toque_nivel = "clamp((60 - esp.touch) / 45, 0, 1)"   # 60 sem dedo → 0; 15 com o dedo → 1

[alarms.esp_offline]
when     = "!good(esp.uptime)"
delay_on = "5s"
severity = "high"
message  = "ESP32 sem comunicação"

[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media", botao = "esp.button", led_estado = "esp.led", ajuste = "calc.toque_nivel" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

- `esp.touch` não tem unidade (`1`), e por isso os literais `60` e `45` são aceites e o `clamp(…, 0, 1)` também (documento 16, §4). A porta `ajuste` é `unit = "1"`: casa.
- **Qualidade nas expressões:** o `calc` tem a pior qualidade dos operandos (16 §3.2). Com o ESP32 em baixo, `calc.toque_nivel` fica `bad(comm_failure)`, e nunca um número inventado.
- **O alarme `esp_offline`** vigia `esp.uptime`, o sinal que muda sempre enquanto o ESP32 está vivo. `good(x)` é uma função meta (documento 02, §10): devolve `false` com qualidade boa quando `x` não é `good`, e por isso o alarme não fica `unknown` justamente quando o equipamento cai.

**O que o `jk check` diz.** Um reflexo comum é pôr o mesmo `stuck_detect` no botão:

```toml
[signals."esp.button"]
stuck_detect = "10m"
```

```
error[J0176]: `stuck_detect` não se aplica a sinais `bool`
  --> joker.toml:14:1
   |
14 | stuck_detect = "10m"
   | ^^^^^^^^^^^^^^^^^^^^ `esp.button` é `bool`: um botão parado é o estado normal
   = ajuda: para um `bool`, usa `debounce`; para saber se o ESP32 está vivo, vigia `esp.uptime`
```

Retira essas linhas.

## 3. A app, versão 0.4

O toque desloca o limite quente: sem dedo, 55 °C; com o dedo, 80 °C (`t_quente` − 15 K + 25 K × `ajuste`). E a app fica **tolerante a falhas do ESP32**: sem ele, o sistema continua a piscar pelo Pi.

```rust
// apps/luz/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "luz", version = "0.4.0")]
pub struct Luz {
    #[output]             led: Output<bool>,          // pi.act_led
    #[output]             led_estado: Output<bool>,   // esp.led
    #[input(unit = "°C")] temp: Input<f64>,           // calc.temp_media
    #[input]              botao: Input<bool>,         // esp.button
    /// 0 = sem dedo, 1 = dedo em cima (calc.toque_nivel).
    #[input(unit = "1")]  ajuste: Input<f64>,

    #[param(unit = "ms", default = 500.0)] half_period: Param<f64>,
    #[param(unit = "°C", default = 45.0)]  t_frio: Param<f64>,
    #[param(unit = "°C", default = 70.0)]  t_quente: Param<f64>,
}

/// O toque desloca o limite quente entre t_quente − 15 K e t_quente + 10 K.
const RECUO_K: f64 = 15.0;
const FAIXA_K: f64 = 25.0;
/// O ajuste que dá exatamente o t_quente do manifesto: é o "último bom" antes da primeira leitura.
const AJUSTE_NEUTRO: f64 = RECUO_K / FAIXA_K; // 0.6

/// `persist`: o estado passa por uma troca a quente e entra no `jk record` (§7).
#[joker::state(persist)]
struct State { ligado: bool, premido: bool, ajuste: f64, decorrido_ms: f64, aceso: bool }

impl Default for State {
    fn default() -> Self {
        // Arranca ligado, como a 0.3.
        State { ligado: true, premido: false, ajuste: AJUSTE_NEUTRO, decorrido_ms: 0.0, aceso: false }
    }
}

#[joker::cycle(period = "20ms", wcet = "20us")]
fn pisca(io: &mut CycleIo<Luz>, st: &mut State) -> Result<()> {
    // 1. Botão. Sem qualidade conta como não premido: get_or(false) nunca cria um flanco.
    let premido = io.botao.get_or(false);
    if premido && !st.premido {
        st.ligado = !st.ligado;
        st.decorrido_ms = 0.0;
        st.aceso = st.ligado;
    }
    st.premido = premido;
    io.led_estado.set(st.ligado);

    // 2. Ajuste. Sem qualidade, fica o último bom; cada substituição fica no traço.
    st.ajuste = io.ajuste.get_or(st.ajuste);

    if !st.ligado {
        io.led.set(false);
        return Ok(());
    }

    // 3. Temperatura. Sem ela não há ritmo honesto: `?` → Err → lease expira → safe (como na 0.2).
    let temp = io.temp.get()?;
    let t_frio = io.t_frio.get();
    let t_quente = io.t_quente.get() - RECUO_K + FAIXA_K * st.ajuste;
    let x = ((temp - t_frio) / (t_quente - t_frio).max(1.0)).clamp(0.0, 1.0);
    let meio_ms = io.half_period.get() - x * (io.half_period.get() - 100.0);

    st.decorrido_ms += io.dt().as_secs_f64() * 1000.0;
    if st.decorrido_ms >= meio_ms {
        st.decorrido_ms -= meio_ms;
        st.aceso = !st.aceso;
    }
    io.led.set(st.aceso);
    Ok(())
}
```

Três entradas, três políticas, todas escritas no código:

| Entrada | Chamada | Sem qualidade | Porquê |
|---|---|---|---|
| `temp` | `get()?` | a tarefa devolve `Err`; as propostas já feitas ficam (ref/04 §2.6.1), o lease do `led` expira → `safe` | sem temperatura, o ritmo do pisca seria uma mentira |
| `botao` | `get_or(false)` | não premido: não há flancos | um botão que não se vê não carregou em nada |
| `ajuste` | `get_or(último bom)` | o valor anterior, **marcado** | é conforto; o último valor é a melhor estimativa |

`get_or` nunca esconde nada: a tabela do documento 16 §3.1 manda registar no traço que a leitura foi substituída (`note-substituted`, ref/04 §2.3). Uma amostra `bad` não se converte em número em silêncio (invariante 4 do `AGENTS.md`).

## 4. A falha no simulador

Confirma que o `sim/esp.sim.toml` tem a tecla de falha (a parte 4 deixou-a preparada; se não, acrescenta-a):

```toml
# sim/esp.sim.toml (acrescentar)
[faults]
panel = { key = "d", disconnect = true }   # "d" desliga o ESP32 inteiro (ref/03 §2.4.8)
```

```sh
$ jk dev --env sim --panel     # "b" liga, o slider do toque mexe o ritmo, "d" desliga o ESP32
```

Agora o teste. Este cenário é o contrato de tolerância a falhas da trilha: **vai passar sem alterações nas versões 0.4, 0.5 e 0.6 da app**. Por isso só fala de sinais dos dispositivos e do alarme, nunca de detalhes internos da app ou dos `calc`.

```toml
# sim/scenarios/falha_esp.scenario.toml
schema = 1

[scenario]
name     = "o ESP32 cai a meio: amostras bad, alarme, o Pi continua a piscar"
manifest = "joker.sim.toml"
duration = "60s"
tags     = ["falhas"]

# Condições fixas: CPU a 60 °C e toque a 42 → ajuste 0,4 → limite quente 70 − 15 + 10 = 65 °C
# → x = (60 − 45) / (65 − 45) = 0,75 → meio período = 500 − 0,75 × 400 = 200 ms.
# (0,4 não é o AJUSTE_NEUTRO: um valor de recurso fixo daria outro ritmo.)
# O sistema arranca ligado: não há pressão inicial.
[[set]]
at     = "0s"
signal = "pi.cpu_temp"
value  = "60 °C"

[[set]]
at     = "0s"
signal = "esp.touch"
value  = 42

[[expect]]
during        = "5s .. 19s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
during = "5s .. 19s"
signal = "esp.led"
equals = true

[[expect]]
at     = "19s"
alarm  = "esp_offline"
active = false

# A falha: o ESP32 inteiro deixa de responder durante 20 s.
[[inject]]
at         = "20s"
device     = "esp"
disconnect = true
for        = "20s"

# Uma pressão durante a falha não conta: o botão não se vê.
[[press]]
signal = "esp.button"
at     = "30s"

[[expect]]
during  = "21s .. 39s"
signal  = "esp.uptime"
quality = "bad(comm_failure)"

[[expect]]
during        = "21s .. 39s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
at     = "24s"
alarm  = "esp_offline"
active = false                   # ainda dentro do delay_on

[[expect]]
within = "25s .. 26s"
alarm  = "esp_offline"
active = true

[[expect]]
at     = "39s"
alarm  = "esp_offline"
active = true

# Recuperação: o sistema continua ligado, e o LED do ESP32 volta a mostrá-lo.
[[expect]]
during = "41s .. 60s"
signal = "esp.led"
equals = true

[[expect]]
during        = "41s .. 60s"
signal        = "pi.act_led"
toggles_every = "200ms ± 20ms"

[[expect]]
at     = "42s"
alarm  = "esp_offline"
active = false
```

Repara em três escolhas. O toque a 42 dá um ritmo que depende do ajuste (a 45 °C, o ajuste não contaria) e que não coincide com nenhum valor de recurso fixo. O ritmo **durante** a falha é igual ao de antes: só o `get_or(último bom)` o consegue. E a pressão aos 30 s prova que o botão sem qualidade não liga nem desliga nada: aos 41 s o sistema ainda está ligado.

```sh
$ jk test sim/scenarios/falha_esp.scenario.toml --env sim
```

```
cenário falha_esp · "o ESP32 cai a meio: …" · 60 s virtuais em 0,8 s
  ✓ 11 expectativas
```

## 5. Na bancada: desligar o cabo

```sh
$ jk build
$ jk lock --env lab --node lab
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
```

O sistema arranca ligado; escolhe um ritmo com o dedo. Depois **desliga o cabo micro-USB do ESP32**.

O cabo leva duas coisas: a porta série **e a alimentação**. Sem ele, o ESP32 fica sem corrente:

- o LED do ESP32 apaga-se **logo**. Isto não é o fallback: é falta de corrente;
- a porta `/dev/serial/by-id/usb-Silicon_Labs_CP2102_…` desaparece do Pi 3, e o driver declara o dispositivo em baixo;
- quando voltares a ligar, o ESP32 **arranca de novo**: `esp.uptime` recomeça do zero (é `monotonic`: o decréscimo conta na métrica `joker_counter_resets_total`, e é assim que se distingue "reiniciou" de "o cabo falhou"), o Wi-Fi volta a ligar, e o LED fica apagado até o Joker reenviar o último valor aplicado (o *resync*, ref/06 §2.1).

```sh
$ jk status --node lab
```

```
→ lab (dev) · de --node
geração       5 · luz 0.4.0 · ativa
grupos        main 20 ms · overruns 0
dispositivos  pi    sysfs        up
              esp   modbus-rtu   down   bad(comm_failure) há 9 s · porta ausente
alarmes       esp_offline (high) ativo há 4 s · não reconhecido · "ESP32 sem comunicação"
apps          luz 0.4.0 a correr
saídas        pi.act_led   control:12 · a piscar
              esp.led      control:12 true · envio a falhar (comm_failure)
```

O LED do Pi continua a piscar, ao mesmo ritmo. Segue as duas pontas da falha:

```sh
$ jk trace calc.toque_nivel --node lab
```

```
calc.toque_nivel = clamp((60 - esp.touch) / 45, 0, 1)
  esp.touch       bad(comm_failure)    último bom 22 há 9,4 s
  calc            bad(comm_failure)
  lido por        luz.pisca, porta ajuste
    get_or → 0.844    substituted    (a cada tick desde 10:14:05.140)
```

```sh
$ jk trace esp.led --node lab
```

```
esp.led   saída · esp (modbus-rtu, bus esp32) · coil 0, FC 5
  proposta   luz.pisca · control:12 · true
  árbitro    control:12 ativo · lease 60 ms
  envelope   accepted
  commit     true → caixa do driver (latest-wins)
  driver     falhou: comm_failure   desde 10:14:05.140
  readback   esp.led_rb bad(comm_failure)
  fallback   local/bench-esp32@1: sem pedidos durante 2 s, o equipamento apaga (value = false)
```

E o registo de decisões (M3), que guarda cada escrita com o resultado e as entradas de que dependia:

```sh
$ jk decisions --output pi.act_led --cause calc.toque_nivel --since 2m --node lab
```

```
10:14:05.160  control:12  true   accepted   ajuste = 0.844 (substituted) · temp = 58,1 °C
10:14:05.420  control:12  false  accepted   ajuste = 0.844 (substituted) · temp = 58,1 °C
…
```

Cada decisão tomada com um valor substituído diz que o foi. Volta a ligar o cabo: em poucos segundos o dispositivo passa a `up`, o alarme deixa de estar ativo e o LED do ESP32 volta a mostrar o estado.

## 6. O fallback do equipamento, visto a sério

Com o cabo desligado não se vê o fallback: o ESP32 está sem corrente. Para o ver, o ESP32 tem de continuar alimentado e deixar de ouvir perguntas. Na parte 4 mataste o `jokerd` inteiro; agora para só o dispositivo:

```sh
$ jk device stop esp --reason "ver o fallback do ESP32" --node lab
```

```
→ lab (dev) · de --node · lab.local:7443
esp: stopped (fecho ordenado; volátil: até `jk device start esp`, ao próximo arranque do jokerd ou à próxima geração)
```

- ao fim de **2 s** sem pedidos Modbus, o firmware de bancada apaga o LED do ESP32 sozinho. É o `[fallback]` do perfil (`timeout = "2s"`, `value = false`), o M1 do documento 01, §8;
- o `jokerd` continua a correr, e o `ACT` continua a piscar ao mesmo ritmo. Os sinais `esp.*` ficam `bad(not_connected)`, e não `bad(comm_failure)`: foi o software que parou, e não o cabo (ref/06 §2.8). Para a `luz` é igual, e o `esp_offline` dispara ao fim de 5 s;
- parar um dispositivo é do papel `operator` e fica na auditoria (`POST /v1/devices/esp/stop`, ref/09 §5.12).

```sh
$ jk device start esp --node lab
```

**Porque é que o fallback nunca dispara em funcionamento normal**, mesmo quando o LED não muda durante minutos? Porque o núcleo renova a escrita a cada *keepalive*, que por omissão é 1/5 do timeout do equipamento, lido do lock: 400 ms. O deadband e o `min_interval` nunca suprimem um keepalive (documento 01, §8). O `jk check` confirma que a conta fecha (`J0276`: o timeout do equipamento tem de ser maior do que o keepalive mais o tempo de reinício do `jokerd`).

E repara que o fallback do equipamento (`false`) é igual ao `safe` da saída (`[outputs."esp.led"] safe = false`). Se fossem diferentes, era `J0205`: quando o Joker cai, o equipamento tem de ir para o mesmo lado seguro que o Joker escolheria.

## 7. Gravar a falha e reproduzi-la no portátil

Começa a gravar, com o sistema como estiver. A gravação corre no Pi 3, e o ficheiro vem para o portátil no fim:

```sh
$ jk record --for 5m --out falha.jktrace --node lab
```

Durante os 5 minutos: desliga e volta a ligar o sistema com o `BOOT`, mexe no toque, desliga o cabo do ESP32 durante uns 40 s, volta a ligá-lo.

```
→ lab (dev) · de --node
a gravar  grupo main (20 ms) · 3 entradas, 2 saídas · geração 5 (luz 0.4.0)
estado    luz.pisca: export-state entre dois ticks (segmento de estado, 41 B)
gravado   5m00s · 15 000 ticks · 2,0 MB (0,3 MB comprimido) → falha.jktrace
```

Primeiro, a prova de determinismo: a mesma app, sobre o mesmo traço, dá os mesmos bytes.

```sh
$ jk replay falha.jktrace --diff
```

```
traço      falha.jktrace · nó lab · geração 5 · luz 0.4.0 · 15 000 ticks
reexecução luz 0.4.0 (a gravada) · a partir do estado gravado
resultado  0 diferenças: propostas e resultados iguais byte a byte
```

Agora olha para o que o traço mostra durante a falha: a app continuou a propor `esp.led = true` a cada tick, o envelope aceitou, e o Joker ficou com `true` como valor aplicado, enquanto o LED real estava apagado (sem corrente, ou pelo fallback). Nada de perigoso, mas o Joker e o equipamento contam histórias diferentes. A correção é pequena: a app só propõe o LED do ESP32 quando vê o ESP32. Sem proposta, o lease expira e a saída vai para o `safe`, que é `false`: o mesmo que o equipamento faz sozinho.

```rust
// apps/luz/src/lib.rs — o que muda da 0.4.0 para a 0.4.1
#[joker::app(name = "luz", version = "0.4.1")]
// …
    // O LED do ESP32 só se propõe quando se vê o ESP32 (o botão vem do mesmo equipamento).
    // Sem proposta, o lease expira e a saída vai para o safe (false), o valor do fallback do equipamento.
    if io.botao.sample().quality.is_good() {
        io.led_estado.set(st.ligado);
    }
```

```sh
$ jk build
$ jk replay falha.jktrace --diff --app luz@0.4.1
```

```
traço      falha.jktrace · nó lab · geração 5 · luz 0.4.0 · 15 000 ticks
reexecução luz 0.4.1 (apps/luz · sha256:5be1…) · estado gravado, pelo migrate

saída        ticks diferentes   de           até
pi.act_led   0                  —            —
esp.led      1 963              1m42,380s    2m21,640s

esp.led · tick 5 119 (1m42,380s)
  gravado   proposta luz control:12 true → accepted · aplicado true
  novo      sem proposta · lease de luz até 1m42,440s · aplicado true
esp.led · tick 5 122 (1m42,440s)
  gravado   proposta luz control:12 true → accepted · aplicado true
  novo      sem escritor ativo → safe · aplicado false
```

A diferença está só onde devia: no `esp.led`, só durante a falha. O LED do Pi, em 15 000 ticks, não mudou um bit. É isto que se leva para uma revisão antes de uma atualização. O cenário do §4 passa igual com a 0.4.1 (não diz nada sobre o `esp.led` durante a falha), e a 0.4.1 vai para o lab:

```sh
$ jk test --env sim
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
$ jk bench --env lab --node lab     # a app mudou de digest: o WCET antigo deixou de valer (J0431)
```

O `.jktrace` não vai para o Git: é um dado, não é código.

## 8. O que aconteceu por baixo

| Nesta parte | Onde está |
|---|---|
| deadband e `stuck_detect` são do estágio de aquisição, por esta ordem, antes de qualquer app | documento 01 §3.3; ref/03 §3 ("Aquisição") |
| um `calc` herda a pior qualidade dos operandos; `good(x)` é meta | documento 16 §3.2; documento 02 §10 |
| `get_or` devolve o valor de recurso e deixa uma nota no traço; `get()?` numa tarefa de ciclo mantém as propostas já feitas | documento 16 §3.1; ref/04 §2.3, §2.6.1 |
| dispositivo em baixo → `bad(comm_failure)` em todos os pontos; driver morto seria `bad(not_connected)` | ref/06 §2.8 (qualidade que vem do link) |
| keepalive, fallback do equipamento (M1) e a coerência com o `safe` (`J0205`, `J0276`) | documento 01 §8; ref/01 §5.8 |
| cada escrita, com resultado, causa e entradas | M3; documento 07 §4 (`jk decisions`) |
| a falha no cenário (`[[inject]] device … disconnect`) e as expectativas `quality`, `active`, `toggles_every` | ref/03 §2.5.3, §2.5.4 |

**Determinismo: o relógio é uma entrada.** Cada tick do `.jktrace` guarda `t_mono`, `t_utc` e a qualidade do relógio, as entradas de cada porta do grupo (16 B cada), as propostas e os resultados (documento 16, §7). Na reexecução, `io.now()` e `io.dt()` vêm do traço, e não do relógio do portátil (ref/04 §2.7: os do latch); o núcleo não lê o relógio, recebe-o (invariante 1, ADR 0007). O árbitro e o envelope que correm no replay são o mesmo código que corre no Pi. Por isso a mesma app sobre o mesmo traço dá **os mesmos bytes** (porta de saída da Fase 0), e uma app diferente dá diferenças que se podem contar tick a tick.

**O estado de partida.** O `jk record` guarda, logo a seguir ao cabeçalho, o `export-state` de cada app com estado `persist` (o mesmo da troca a quente, ref/04 §4.12), e o `jk replay` começa desse estado, e não do zero: é por isso que podes começar a gravar com o sistema ligado ou desligado (documento 16, §7; documento 04, §7). Com `--app luz@0.4.1`, o estado da 0.4.0 entra pelo `migrate`, que casa os campos por nome. Um traço do anel contínuo (`jk trace-dump`) não tem esse segmento: aí o replay começa do `init`, e os primeiros ticks podem divergir do gravado até o estado convergir (o `--diff` marca-os "antes do estado").

## 9. Exercícios

1. Muda o `calc.toque_nivel` para `clamp((60 - avg(esp.touch, 500ms)) / 45, 0, 1)`. O ritmo deixa de tremer quando o dedo está meio encostado? Quantas linhas mudam na app? (Resposta: zero.)
2. Escreve um cenário em que o CPU passa de 60 °C a `bad(comm_failure)` durante 5 s (`[[inject]] signal = "pi.cpu_temp"`). Que política de qualidade da tabela do §3 decide, e o que faz o `pi.act_led`? Usa `level = "none"` numa expectativa.
3. Grava 2 minutos em que só mexes no toque, e reproduz com a 0.4.0 e a 0.4.1. Quantos ticks diferentes esperas? Confirma.
4. Troca o `get_or(st.ajuste)` por `get_or(AJUSTE_NEUTRO)` e corre o `falha_esp.scenario.toml`. Que expectativa falha, com que ritmo visto, e porque é que um toque a 33 no cenário não a teria apanhado?

```sh
$ git add -A && git commit -qm "trilha(05): toque como ajuste, falhas do ESP32, fallback e replay (luz 0.4.1)"
```

**Estado no fim desta parte:** app `luz` 0.4.1, a correr no lab (geração 6; a 0.4.0 foi a 5). Novos ou mudados: `joker.toml` (`[signals."esp.touch"]`, `calc.toque_nivel`, `[alarms.esp_offline]`, porta `ajuste`), `sim/esp.sim.toml` (tecla de falha), `sim/scenarios/falha_esp.scenario.toml`, `apps/luz/`, `joker.lab.lock`.

**Próximo:** [Parte 6 — Trocar de placa: o mesmo projeto no Pi Zero W, e a placa que ainda não existe](06-trocar-de-placa.md).
