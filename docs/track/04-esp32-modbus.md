# Parte 4 — O ESP32 como equipamento Modbus: escrever o perfil, o botão e o LED

**Objetivo:** pôr o ESP32 a fazer de equipamento Modbus RTU ligado ao Pi 3, escrever o seu perfil a partir do mapa de registos, e usar o botão `BOOT` para ligar e desligar o sistema, com o LED do ESP32 a mostrar o estado.

**Vais aprender:** Modbus RTU numa ponte USB-série · perfis de equipamento, do CSV ao ficheiro revisto · `jk profile import`, `check` e `test` · readback e `safe_hint` · fallback do equipamento (M1) · flancos e debounce · orçamento do barramento · `jk lock --node`.

**Antes de começar:** a parte 3 feita (`luz` 0.2 a piscar com a temperatura). O ESP32 ligado por micro-USB a uma porta USB do Pi 3. No portátil, a toolchain Rust para Xtensa (`espup`) e o `espflash`.

---

## 1. O que o ESP32 vai ser

Nesta parte o ESP32 ainda não corre o Joker (isso é a parte 9). Corre o **firmware de bancada** do documento 23, §4: um equipamento Modbus pequeno, como um medidor ou um relé com comunicação, com esta tabela de registos:

| Registo | FC | Tipo | Sinal |
|---|---|---|---|
| coil 0 | 1 (ler), 5 (escrever) | `bool` | LED da placa (GPIO2) |
| discrete input 0 | 2 | `bool` | botão `BOOT` (GPIO0), já invertido: 1 = premido |
| input register 0 | 4 | `u16` | leitura do sensor de toque T0 (GPIO4) |
| input register 1 | 4 | `i16` | RSSI do Wi-Fi em dBm (0 sem Wi-Fi) |
| input registers 2–3 | 4 | `u32` | segundos desde o arranque |
| holding register 0 | 3 | `u16` | versão do firmware de bancada |

E um comportamento que qualquer equipamento sério tem: **sem nenhum pedido Modbus durante 2 s, apaga o LED sozinho.** É o fallback do equipamento (M1, documento 01, §8), e é a lição desta parte.

A ponte USB-série da placa (CP2102 ou CH340) é uma porta série real para o Pi. Não é preciso transceptor RS-485 nem fios: o cabo micro-USB dá alimentação e o barramento (documento 23, §2).

## 2. Gravar o firmware

O firmware está em `tools/bench/esp32-modbus/` no repositório do Joker. Serve Modbus RTU no UART0 (o USB) e Modbus TCP no Wi-Fi (porta 502), com a mesma tabela; o Wi-Fi só vai ser preciso na parte 6, mas grava-se já tudo de uma vez. As credenciais entram no build por variáveis de ambiente e nunca vão para o Git:

```sh
$ git clone https://git.example.com/joker/joker.git && cd joker/tools/bench/esp32-modbus
$ export BENCH_WIFI_SSID="bancada"
$ read -rs BENCH_WIFI_PASSWORD && export BENCH_WIFI_PASSWORD      # sem eco, fora do histórico
$ cargo build --release
$ espflash flash --monitor target/xtensa-esp32-none-elf/release/esp32-modbus
```

```
esp32-modbus 1.0.0 · Modbus RTU no UART0, 115200 8N1, unidade 1
wifi: ligado a "bancada", IP 192.168.1.57 · Modbus TCP em :502
fallback: LED apaga sem pedidos durante 2 s
uart0: a partir daqui, só Modbus
```

A última linha importa: o UART0 é a consola e também o barramento, e depois do arranque o firmware deixa de escrever nele. Aponta o IP (parte 6). O binário tem a palavra-passe do Wi-Fi lá dentro: não o partilhes nem o guardes no projeto.

## 3. O ESP32 visto do Pi

Ligado ao Pi, o ESP32 aparece em `/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0` (`lab$ ls /dev/serial/by-id/`). O `/dev/ttyUSB0` também existe, mas o número muda se ligares outro adaptador. Usa-se sempre o caminho por `by-id` (numa placa com CH340 é `usb-1a86_USB_Serial-if00-port0`). O utilizador do `jokerd` tem de poder abrir a porta (grupo `dialout`); o `jk doctor --node lab` verifica-o.

## 4. O perfil: do mapa de registos ao ficheiro

O Modbus não se descreve a si próprio: o que cada registo quer dizer está na ficha técnica, e no Joker essa ficha passa a ser um **perfil** (ref/03 §2.2). Escrever o perfil de um equipamento novo é o trabalho mais comum de um integrador, e faz-se em três passos: importar, rever, testar.

### 4.1 Do CSV ao rascunho

A tabela do §1, copiada para uma folha de cálculo e exportada para `mapa-esp32.csv`, na raiz do projeto. O cabeçalho é obrigatório e fixo, e as colunas vazias ficam com a omissão:

```
signal,fc,at,type,direction,unit,scale,offset,label,description
led,5,0,bool,output,,,,,
led_rb,1,0,bool,,,,,,
button,2,0,bool,,,,,BOOT,
touch,4,0,u16,,,,,,valor cru do sensor de toque T0 (GPIO4); desce quando se toca
rssi,4,1,i16,,,,,,RSSI do Wi-Fi em dBm; 0 sem ligação
uptime,4,2,u32,,s,,,,
fw,3,0,u16,param,,,,,
```

- `led` é o coil 0 **escrito** com FC 5; `led_rb` é o mesmo coil **lido** com FC 1. Um perfil separa a escrita da leitura de retorno: é o único caso de dois pontos no mesmo endereço (ref/03 §2.2.5).
- O `at` é o endereço do protocolo, com base 0. Uma ficha que diga "30001" quer dizer input register 0.

```sh
$ jk profile import csv mapa-esp32.csv --name local/bench-esp32 --driver modbus --out profiles/local/bench-esp32@1.profile.toml
```

```
rascunho escrito em profiles/local/bench-esp32@1.profile.toml: 7 sinais (1 saída, 5 entradas, 1 param)
rever: [profile] description · [defaults] poll · saída `led` sem readback nem safe_hint ·
       sem [fallback] · deadband, limits e monotonic não vêm do CSV
```

`local/` é o espaço dos perfis do projeto, que nunca se publicam (ref/03 §2.1.2). O importador só sabe o que a tabela diz. O resto é conhecimento sobre o equipamento, e é teu.

Repara no nome: `<modelo>@<major>.profile.toml`. É a mesma regra do `joker.sim.toml` e do `sim/pi.sim.toml` da parte 1: o sufixo diz o tipo, e por isso o editor abre o ficheiro já com o esquema de perfil (completação das chaves de `[signals]`, erros enquanto escreves), e o `jk check` recusa com `J0803` um `.profile.toml` que não comece por `[profile]`. O manifesto continua a referi-lo sem sufixo, `profile = "local/bench-esp32@1"`: é a resolução que o acrescenta (ADR 0026).

### 4.2 Rever à mão

O que se acrescenta ao rascunho, e porquê:

| Onde | O quê | Porquê |
|---|---|---|
| `[profile]` | `description`, `license` | para quem ler o ficheiro daqui a um ano |
| `[profile]` | `identity = { firmware = "fw" }` | este Modbus não se descreve a si próprio: o registo `fw` passa a ser a versão do firmware do equipamento, e o lock guarda-a |
| `[defaults]` | `poll = "100ms"` | um botão lido a 1 s (a omissão) não serve; 100 ms é o ritmo de uma pessoa |
| `led` | `readback = "led_rb"` | sugere ao manifesto o `verify` da saída (o `jk check` mostra-o) |
| `led` | `safe_hint = false` | sugestão para o `safe`, mostrada ao lado do `J0201`; **nunca é aplicada** (ADR 0006) |
| `touch` | `deadband = 5` | o toque é ruidoso: variações de ±5 contagens não são sinal |
| `uptime` | `monotonic = true` | é um contador: o historian agrega-o pelo último valor, e não pela média |
| `[fallback]` | `timeout = "2s"`, `value = false` | o que o firmware faz sozinho; o Joker compara-o com o `safe` |

O perfil final:

```toml
# profiles/local/bench-esp32@1.profile.toml
schema = 1

[profile]
name        = "local/bench-esp32"
version     = "1.0.0"
driver      = "modbus"
description = "Firmware de bancada do ESP32 (documento 23, §4): LED, botão, toque, RSSI"
license     = "Apache-2.0"
identity    = { firmware = "fw" }

[defaults]
poll = "100ms"

[signals]
"led"     = { at = 0, fc = 5, type = "bool", direction = "output", readback = "led_rb", safe_hint = false }
"led_rb"  = { at = 0, fc = 1, type = "bool" }
"button"  = { at = 0, fc = 2, type = "bool", label = "BOOT" }
"touch"   = { at = 0, fc = 4, type = "u16", deadband = 5, description = "valor cru do sensor de toque T0 (GPIO4); desce quando se toca" }
"rssi"    = { at = 1, fc = 4, type = "i16", description = "RSSI do Wi-Fi em dBm; 0 sem ligação" }
"uptime"  = { at = 2, fc = 4, type = "u32", unit = "s", monotonic = true }
"fw"      = { at = 0, fc = 3, type = "u16", direction = "param" }

[fallback]
timeout = "2s"
value   = false
```

É, sinal a sinal, o perfil do documento 23, §4, com o nome do projeto. Repara nos tipos que resultam (ref/03 §2.2.7): `touch` e `fw` ficam `u64`, `rssi` fica `i64`, e `uptime`, por ter unidade, fica `f64` em segundos. O `rssi` é logarítmico e não leva unidade: a descrição diz que é dBm. O `fw` é um `param`, e por isso lê-se só a pedido (`jk read esp.fw --refresh`), e não em cada poll. Sem `applies_to`, o `[fallback]` cobre as saídas do perfil, e aqui há só uma.

### 4.3 `jk profile check`

Um erro clássico ao rever à mão é copiar o FC de leitura para a saída (`"led" = { at = 0, fc = 1, …, direction = "output" }`):

```
error[J0810]: endereço ou código de função inválido para a direção
  --> profiles/local/bench-esp32@1.profile.toml:14:13
   |
14 | "led"     = { at = 0, fc = 1, type = "bool", direction = "output", … }
   |                       ^^^^^^ FC 1 lê coils; uma saída escreve-se com FC 5 (um coil) ou 15 (vários)
   = ajuda: fc = 5; para ler o valor de volta, um ponto de entrada no mesmo coil com fc = 1 (`led_rb`)
```

Com o `fc = 5`:

```sh
$ jk profile check profiles/local/bench-esp32@1.profile.toml
```

```
✓ esquema          7 sinais: 1 saída, 5 entradas, 1 param; [fallback] sobre `led`; identity.firmware = fw
✓ endereços        coil 0 (escrita e retorno) · discrete 0 · input 0–3 · holding 0; sem sobreposições
✓ tipos            touch u16→u64 · rssi i16→i64 · uptime u32→f64 s · fw u16→u64
✓ pedidos (plan)   por poll: FC1 × 1 bit, FC2 × 1 bit, FC4 × 4 registos · a pedido: FC3 × 1 registo
```

O `check` não precisa do equipamento: valida o esquema, as unidades e o `plan` do driver Modbus, que é puro (ref/03 §3).

### 4.4 `jk profile test` no hardware

Agora contra o ESP32 real, pelo Pi, só com leituras:

```sh
$ jk profile test profiles/local/bench-esp32@1.profile.toml --node lab \
     --uri "modbus-rtu:/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0?baud=115200&unit=1"
```

```
sinal    registo    cru      valor     qualidade  latência
led_rb   coil[0]    0        false     good       4 ms
button   di[0]      0        false     good       4 ms
touch    in[0]      61       61        good       5 ms
rssi     in[1]      0xFFCA   -54       good       (mesmo pedido)
uptime   in[2..3]   1204     1204 s    good       (mesmo pedido)
fw       hr[0]      1        1         good       4 ms
led      coil[0]    —        (saída: um teste não escreve)
3 pedidos por poll + 1 a pedido · latência medida 4–5 ms (o perfil assume a omissão, 10 ms)
```

Corre-o outra vez com o `BOOT` premido (`button` a `true`) e outra com um dedo no pino GPIO4 (`touch` desce para ~15). Os três números batem com o mundo, e o perfil está certo.

## 5. O ESP32 no simulador

Um modelo de simulação pode reutilizar os sinais de um perfil (`[model] profile = "local/bench-esp32@1"`, ref/03 §2.4). Aqui repetem-se de propósito, com os mesmos nomes e os tipos que o núcleo vê, porque o modelo precisa de dizer mais do que o perfil: o painel, o ruído, o ressalto e a dinâmica.

```toml
# sim/esp.sim.toml
schema = 1

[model]
description = "ESP32 com o firmware de bancada (documento 23, §4)"

[signals]
led    = { type = "bool", direction = "output", panel = { label = "LED ESP32", kind = "lamp" } }
led_rb = { type = "bool" }
button = { type = "bool", init = false, bounce = "5ms", panel = { label = "BOOT", kind = "momentary", key = "b" } }
touch  = { type = "u64", init = 60, noise = 1, resolution = 1,
           panel = { label = "Toque", kind = "slider", keys = ["down", "up"], step = 5, min = 0, max = 100 } }
rssi   = { type = "i64", gen = { kind = "noise", mean = -55, std = 3 } }
uptime = { type = "f64", unit = "s", init = 0 }
fw     = { type = "u64", direction = "param", init = 1 }

[dynamics]
led_rb = "led"                     # o retorno segue o valor aplicado
uptime = "integrate(1)"            # 1 s por segundo

[faults]
panel = { key = "d", disconnect = true }   # "d" simula o ESP32 desligado (parte 5)
```

```toml
# joker.sim.toml (acrescentar)
[devices.esp]
driver = "sim"
model  = "sim/esp.sim.toml"
```

O `bounce = "5ms"` põe o ressalto de um botão real no portátil, para o bug aparecer aqui e não só na bancada.

## 6. O manifesto comum

```toml
# joker.toml (acrescentar)
[outputs."esp.led"]
safe = false

[signals."esp.button"]
debounce = "20ms"

[apps.luz]
path   = "apps/luz"
bind   = { led = "pi.act_led", temp = "calc.temp_media", botao = "esp.button", led_estado = "esp.led" }
params = { half_period = "500 ms", t_frio = "45 °C", t_quente = "70 °C" }
cycles = { pisca = "main" }
```

Esquece o `[outputs."esp.led"]` e o `jk check` lembra-te, com a sugestão do perfil:

```
error[J0201]: saída sem `safe`
  --> joker.toml:26:1
   = nota: `esp.led` é uma saída do perfil local/bench-esp32@1
   = ajuda: o perfil sugere `safe = false` (safe_hint); escreve em [outputs."esp.led"] o valor que é seguro neste site
```

O `debounce` fica no comum, para os dois mundos se comportarem igual. O driver Modbus não faz debounce, e por isso é o estágio de aquisição que o faz (ref/06 §2.4): num `bool` lido por poll, o valor só muda depois de leituras consecutivas iguais que cubram pelo menos o `debounce` (ref/01, chave `debounce`; documento 16). No simulador, apaga o ressalto de 5 ms; no hardware, com um poll de 100 ms, são duas leituras seguidas iguais, e uma leitura isolada a meio de um ressalto não passa. O custo é uma leitura de atraso: ~100 ms entre carregar e o sistema ver.

## 7. A app, versão 0.3

Regra: cada flanco de subida do botão liga ou desliga o sistema; desligado, o `ACT` fica apagado; o LED do ESP32 mostra se está ligado.

```rust
// apps/luz/src/lib.rs — o que muda face à 0.2
#[joker::app(name = "luz", version = "0.3.0")]
pub struct Luz {
    #[output] led: Output<bool>,
    #[input(unit = "°C")] temp: Input<f64>,
    #[input] botao: Input<bool>,
    #[output] led_estado: Output<bool>,
    // half_period, t_frio, t_quente: como na 0.2
}

#[derive(Default)]
struct Pisca {
    elapsed_ms: f64,
    on: bool,
    ligado: bool,
    premido_antes: bool,
}

fn arranque(_io: &CycleIo<Luz>) -> Pisca {
    Pisca { ligado: true, ..Default::default() }     // arranca ligado, como a 0.2
}

#[joker::cycle(period = "20ms", wcet = "5us", init = arranque)]
fn pisca(io: &mut CycleIo<Luz>, st: &mut Pisca) -> Result<()> {
    // 1. As duas entradas antes de qualquer proposta: sem elas, nada é proposto neste tick.
    let t = io.temp.get()?;
    let premido = io.botao.get()?;

    // 2. Flanco de subida: premido agora, solto no tick anterior.
    if premido && !st.premido_antes {
        st.ligado = !st.ligado;
        st.elapsed_ms = 0.0;
        st.on = st.ligado;                       // ao ligar, acende logo: feedback imediato
    }
    st.premido_antes = premido;
    io.led_estado.set(st.ligado);

    if !st.ligado {
        io.led.set(false);
        return Ok(());
    }
    // 3. O pisca da 0.2, sem mudanças.
    let (frio, quente) = (io.t_frio.get(), io.t_quente.get());
    let x = ((t - frio) / (quente - frio)).clamp(0.0, 1.0);
    let meio = io.half_period.get() + x * (RAPIDO_MS - io.half_period.get());
    st.elapsed_ms += io.dt().as_secs_f64() * 1000.0;
    if st.elapsed_ms >= meio {
        st.elapsed_ms -= meio;
        if st.elapsed_ms > meio {
            st.elapsed_ms = 0.0;
        }
        st.on = !st.on;
    }
    io.led.set(st.on);
    Ok(())
}
```

- **O flanco vive no estado da tarefa** (`premido_antes`), porque o `ligado` é memória da app. O manifesto também tem `rose(cond)` nas expressões, e serve para alarmes e permissivos (documento 02, §10), mas quem decide o que uma pressão faz é a app.
- **Arranca ligado** (`init = arranque`), para que o `pisca.scenario.toml` e o `aquece.scenario.toml` passem sem alterações.
- **`botao` com `get()?`**: sem ESP32, a app deixa de propor nas **duas** saídas, e os dois LEDs vão para `safe`. É uma escolha deliberadamente dura; a parte 5 torna a app tolerante a falhas do ESP32.

## 8. Cenário `botao.scenario.toml`

```toml
# sim/scenarios/botao.scenario.toml
schema = 1

[scenario]
name     = "o BOOT liga e desliga o sistema; o LED do ESP32 mostra o estado"
manifest = "joker.sim.toml"
duration = "8s"

[[expect]]
during = "0.5s .. 1.9s"
signal = "esp.led"
equals = true                        # arranca ligado
[[press]]
signal = "esp.button"
at     = "2s"
for    = "300ms"
[[expect]]
during = "2.2s .. 3.9s"
signal = "esp.led"
equals = false
[[expect]]
during = "2.2s .. 3.9s"
signal = "pi.act_led"
equals = false                       # desligado: ACT apagado
[[press]]
signal = "esp.button"
at     = "4s"
for    = "300ms"
[[expect]]
during        = "4.5s .. 8s"
signal        = "pi.act_led"
toggles_every = "500ms ± 20ms"       # ligado outra vez, a 45 °C
[[expect]]
during  = "1s .. 8s"
signal  = "esp.led"
toggles = 2                          # duas pressões, dois flancos: o ressalto não conta
```

```sh
$ jk test
```

```
apps/luz (cargo test) ........................ ok
sim/scenarios/pisca.scenario.toml ..................... ok
sim/scenarios/aquece.scenario.toml .................... ok (460 s simulados em 0,5 s)
sim/scenarios/botao.scenario.toml ..................... ok (8 s simulados em 14 ms)
```

Tira o `debounce` do `joker.toml` e corre outra vez: o `toggles = 2` falha em algumas pressões (`J0842`), com o instante e a cadeia de decisão no relatório. É o ressalto, apanhado no portátil. No `jk dev --env sim --panel`, a tecla `b` é o `BOOT`.

## 9. No Pi 3

```toml
# joker.lab.toml (acrescentar)
[buses.esp32]
kind = "serial"
port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
baud = 115200
dtr  = "low"            # a omissão, escrita por extenso: abrir a porta não reinicia o ESP32
rts  = "low"

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"
```

Nas placas ESP32, o DTR e o RTS da ponte USB-série estão ligados ao `EN` e ao `GPIO0`: é o *auto-reset* que o `espflash` usa para gravar. Com `"low"` (a omissão, ref/01 §5.6), o `jokerd` abre a porta sem os acionar, e o ESP32 não reinicia cada vez que o nó arranca. `"keep"` deixa-os como o kernel os tiver.

```sh
$ jk check --env lab
```

```
✓ manifesto          2 dispositivos (sysfs, modbus-rtu), 2 saídas, 1 app, 1 grupo, 1 calc, 1 alarme
✓ ligações           luz: led → pi.act_led · temp → calc.temp_media (°C) · botao → esp.button · led_estado → esp.led
✓ perfil             local/bench-esp32@1 · fallback do equipamento: esp.led → false em 2 s (coerente com safe)
✓ barramento esp32   115200 8N1, poll 100 ms: FC1 (14 B), FC2 (14 B), FC4 (21 B)
                     4,3 ms no fio + 3 × 0,3 ms de silêncio + 3 × 10 ms de latência (omissão) ≈ 35 ms
                     + keepalive de esp.led (FC5, 16 B) a cada 400 ms ≈ 3 ms por poll
                     → ocupação 38 % (máximo 80 %)
nota: `esp.led` tem readback sugerido pelo perfil: verify = { readback = "esp.led_rb" }
aviso[W0205]: `pi.act_led` não tem fallback do dispositivo                    (o da parte 2)
aviso[W0451]: o perfil de `esp` declara [fallback], mas o lock não o tem verificado
   = ajuda: jk lock --env lab --node lab
```

Lê o orçamento com calma (documento 05, §5): o que pesa não são os bytes, é a latência do escravo, que o perfil deixa na omissão de 10 ms; o `jk profile test` mediu 4–5 ms. Com um poll de 20 ms, a mesma conta daria perto de 190 %, e o `jk check` recusava (`J0370`).

E repara no que **não** aparece: não há `W0205` para o `esp.led`. Com o `[fallback]` declarado no perfil, essa saída **tem** fallback do equipamento, ao contrário do `ACT` da parte 2. O `W0451` só diz que ainda ninguém o confirmou no nó:

```sh
$ jk lock --env lab --node lab
```

```
joker.lab.lock: perfil local/bench-esp32@1 (1.0.0, path:) · fallback.esp capturado (esp.led → false em 2 s) · discovered.esp firmware "1"
```

```toml
# joker.lab.lock (excerto; o [profiles."local/bench-esp32@1"] tem a forma do [boards] da parte 2)
[fallback.esp]
outputs = ["esp.led"]
timeout = "2s"
value = false
configured = false
verified = "2026-09-25T11:04:12Z"

[discovered.esp]
firmware = "1"
```

O lock fixa o digest do ficheiro do perfil (mudar um registo sem mudar o lock é `J0420`) e o fallback visto no equipamento; `configured = false` porque o firmware tem o fallback fixo, e o núcleo só o lê e compara (ref/02 §2.1.12). O firmware do equipamento vem do `identity` do perfil: o `jk lock --node` leu o `fw` (`1`) e escreveu-o em `[discovered.esp]` (ref/02 §2.1.11). Se um dia o ESP32 aparecer com outro firmware, é a deriva `J0401 kind=firmware`, sem descoberta nenhuma (ref/03 §2.2.2).

```sh
$ jk build --target rpi3b-plus
$ jk apply --env lab --node lab --confirm 5m
$ jk confirm --node lab
$ jk read 'esp.*' --watch --node lab
```

```
esp.led      true      good    (control:12 · luz)
esp.led_rb   true      good    idade 0,07 s
esp.button   false     good    idade 0,07 s
…            (touch 61, rssi -54, uptime 1 391 s: good, idade 0,07 s; fw 1, param, a pedido)
```

Carrega no `BOOT`: o LED do ESP32 apaga-se, o `ACT` também. Outra vez: voltam os dois.

**O fallback, a sério.** Com o sistema ligado (LED do ESP32 aceso), mata o `jokerd` sem lhe dar tempo de pôr nada em `safe`, e impede o systemd de o reiniciar:

```sh
lab$ sudo systemctl kill -s KILL jokerd; sudo systemctl stop jokerd
```

Em 2 s, o LED do ESP32 apaga-se. Ninguém lho mandou: o firmware deixou de receber pedidos e aplicou o seu fallback. O `ACT`, esse, fica como estava, porque o LED de uma placa não tem watchdog (parte 2). É a terceira camada do estado seguro, a funcionar num equipamento e a faltar no outro, exatamente como o `jk check` disse. `sudo systemctl start jokerd` repõe tudo.

## 10. O que aconteceu por baixo

- **O perfil é do modelo de equipamento; o manifesto é do site.** O perfil diz endereços, tipos, escalas, retorno e fallback; o manifesto diz o `safe`, o debounce e as ligações (ref/03 §2.2; ref/01 §5.8.1). O `safe_hint` só sugere: o `safe` é sempre uma decisão escrita de quem instala (ADR 0006).
- **Aquisição.** O driver entrega crus; o estágio de aquisição aplica tipo, deadband e debounce pela ordem da ref/03 §3, e a qualidade do link por cima (documento 05, §12).
- **Barramento.** Um barramento série serializa: um pedido de cada vez, com prioridade às escritas (documento 05, §5). Os quatro input registers leem-se num só pedido, porque são contíguos e do mesmo FC. Uma saída não se lê em ciclo: o seu valor é o aplicado, e o retorno é outro ponto.
- **Escritas.** A app propõe o `esp.led` a cada 20 ms, mas só vai ao fio quando muda, mais o keepalive (01 §8), que mantém o equipamento a ver pedidos mesmo com o LED parado.
- **Fallback (M1).** O núcleo compara o fallback do perfil com o `safe` e o `safer` (`J0205`), o lock fixa-o, e a deriva é `J0401 kind=fallback` (ref/02 §3.1.5).

## 11. Exercícios

1. Com o sistema desligado, escreve `true` no coil 0 pelo Modbus TCP do Wi-Fi (o firmware serve os dois ao mesmo tempo), com uma ferramenta como o `mbpoll`. O LED do ESP32 acende e volta a apagar: em quanto tempo, e quem o apagou? Acrescenta depois `verify = { readback = "esp.led_rb" }` ao `[outputs."esp.led"]` e repete.
2. Com a latência medida no `jk profile test`, acrescenta `quirks = { latency = "5ms" }` a uma **cópia** do perfil (`local/bench-esp32-rapido@1`) e compara a ocupação no `jk check`. Qual é o poll mínimo que cabe em 80 %?
3. Faz com que uma pressão longa (> 2 s) no `BOOT` ponha o sistema em "pisca sempre rápido", até à próxima pressão. Escreve primeiro o cenário.
4. Repete o teste do fallback com o sistema desligado (LED do ESP32 apagado). O que prova, e o que não prova?

```sh
$ git add -A && git commit -qm "trilha(04): ESP32 como equipamento Modbus RTU; perfil local/bench-esp32; luz 0.3"
```

**Estado no fim desta parte:** app `luz` 0.3.0, no lab na geração 4; `profiles/local/bench-esp32@1.profile.toml` (e o `mapa-esp32.csv` de onde saiu); `sim/esp.sim.toml` e o `[devices.esp]` no `joker.sim.toml`; `joker.toml` com `[outputs."esp.led"]`, o debounce de `esp.button` e o `bind` novo; `joker.lab.toml` com `[buses.esp32]` (com `dtr`/`rts`) e `[devices.esp]`; `joker.lab.lock` com o perfil, o `[fallback.esp]` e o `[discovered.esp]`; cenário novo `sim/scenarios/botao.scenario.toml`.

**Próximo:** [Parte 5 — o toque como ajuste, qualidade, falhas, fallback e replay](05-touch-and-faults.md).
