# Parte 10 — Lógica no MCU: uma app nativa e o link por Wi-Fi

**Objetivo:** pôr uma app Rust a correr **dentro** do ESP32, que liga e desliga o LED dele pelo botão `BOOT` sem depender do Pi 3, e depois tirar o link do cabo e passá-lo para o Wi-Fi, cifrado.

**Vais aprender:** apps nativas no Joker Micro (o mesmo SDK, com o backend nativo) · escritores locais e remotos no árbitro do MCU · `jk explain` de uma saída do MCU e o `overridden` · o que o `jk build --target mcu` recusa (`J0720`) e avisa (`J0721`) · `jk bench` no próprio chip · `[net.wifi]` e um segredo cifrado para o MCU · o Joker Link em UDP (`J0179`) · o que muda entre série e Wi-Fi.

**Antes de começar:** a parte 9 feita. O ESP32 com o Joker Micro, ligado ao Pi 3 pelo Joker Link em série, sessão `full`, e a `luz` 0.6 a correr no nó `lab`. A rede Wi-Fi da bancada, com o nome e a password à mão.

---

## 1. Porquê lógica no MCU

Na parte 9, o botão `BOOT` fazia uma viagem longa: o MCU lia-o, enviava-o num `SAMPLES` ao Pi 3, a `luz` via o flanco no tick seguinte, propunha o LED, e o `PROPOSE` voltava pelo cabo. Umas dezenas de milissegundos, e **nada** sem o Pi 3.

Há coisas que devem ficar ao pé do hardware: um reflexo, um encravamento local, uma malha rápida. O Joker Micro corre apps nativas em Rust, com **o mesmo SDK** das apps do nó Linux (documento 19, §5). A diferença está no build: no Linux a app compila para Wasm e corre no wasmtime; no MCU compila para `xtensa-esp32-none-elf` com o backend nativo (`backend-native`, ref/08 §2.11.5) e é ligada ao firmware. Não há sandbox: um *panic* reinicia o MCU, e as saídas ficam no estado de reset.

## 2. A app `reflexo`

```sh
$ cd micro/esp && jk app new reflexo --lang rust
```

```rust
// micro/esp/apps/reflexo/src/lib.rs
use joker::prelude::*;

#[joker::app(name = "reflexo", version = "0.1.0")]
pub struct Reflexo {
    #[input]  botao: Input<bool>,
    #[output] led:   Output<bool>,
    #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f32>,
}

#[derive(Default)]
struct Estado {
    ligado: bool,
    antes: bool,
    desde_ms: f32,          // tempo desde o último toque aceite
}

#[joker::cycle(period = "10ms")]
fn reflexo(io: &mut CycleIo<Reflexo>, st: &mut Estado) -> Result<()> {
    st.desde_ms = (st.desde_ms + io.dt().as_secs_f32() * 1000.0).min(60_000.0);
    let premido = io.botao.get_or(false);            // sem leitura boa, conta como solto
    if premido && !st.antes && st.desde_ms >= io.bloqueio.get() {
        st.ligado = !st.ligado;
        st.desde_ms = 0.0;
    }
    st.antes = premido;
    io.led.set(st.ligado);
    Ok(())
}
```

É a mesma regra da `luz` desde a 0.3 (cada flanco de subida alterna `ligado`), mais um bloqueio de 300 ms contra toques repetidos. Nada nesta app é específico do MCU: compilava igual para Wasm. Um teste com `joker-test` corre no portátil, com relógio virtual, como os da `luz`.

## 3. O manifesto: um escritor local acima do link

Acrescenta ao `micro/esp/joker.toml`:

```toml
[apps.reflexo]
path   = "apps/reflexo"
bind   = { botao = "io.button", led = "io.led" }
cycles = { reflexo = "main" }
```

```sh
$ jk check
```

```
erro[J0210]: dois escritores no mesmo nível da mesma saída
 --> joker.toml:46:1
   |
46 | [apps.reflexo]
   | ^^^^^^^^^^^^^^ app:reflexo escreve io.led em control:12 (a omissão de um escritor único)
   = nota: control:12 está em remote_levels de io.led: é o nível do link, onde escreve a luz do nó lab
   = ajuda: dá ao reflexo outro nível, `writer = { level = "supervisor:8" }`, ou tira control:12 de remote_levels
```

No MCU, cada nível delegado ao link **é** um escritor: o par (link, nível) (documento 16, A12). O reflexo não pode ocupar o nível da `luz`. A decisão de desenho é pô-lo **acima** dela, na faixa `supervisor` (níveis 6–8; o 1 é o mais alto, ref/01 §3.11):

```toml
[apps.reflexo]
path   = "apps/reflexo"
bind   = { botao = "io.button", led = "io.led" }
writer = { level = "supervisor:8" }
cycles = { reflexo = "main" }
```

A lista `remote_levels = ["manual:3", "manual:4", "manual:5", "control:12"]` da parte 9 fica como estava, e é por isso que tudo encaixa:

| Nível no árbitro do MCU | Quem | Ganha a |
|---|---|---|
| `manual:3`–`manual:5` | um `jk write` no nó `lab`, pelo link | tudo o que está abaixo, durante o TTL |
| `supervisor:8` | `app:reflexo`, **local** | a `luz` |
| `control:12` | `app:luz`, remota | só ao `safe` |

O `supervisor:8` não está em `remote_levels`: nenhuma proposta do Pi 3 o pode ocupar (seria `Rejected(NotOwner)`, e no `jk check` do nó `J0731`). E um humano no Pi 3 continua a poder sobrepor-se ao reflexo, porque a faixa `manual` fica acima.

## 4. O que o `jk build --target mcu` não deixa passar

A primeira versão desta app tinha duas coisas a mais. Uma tarefa async para contar os toques da última hora:

```rust
#[joker::task(every = "1m")]
async fn estatistica(io: &TaskIo<Reflexo>) -> Result<()> {
    let h = io.botao.history(io.now() - 1.h(), io.now(), 1.mins()).await?;
    // …
}
```

```
erro[J0720]: `history` não existe no Joker Micro
  --> micro/esp/apps/reflexo/src/lib.rs:34:22
   |
34 |     let h = io.botao.history(io.now() - 1.h(), io.now(), 1.mins()).await?;
   |                      ^^^^^^^ um MCU não tem historian (documento 19, §3)
   = ajuda: uma janela do núcleo num `[calc]` do MCU (`count`, `avg(x, 15m)`), ou fazer a conta no nó Linux,
            onde `esp.button` está no historian
```

O erro nasce no `rustc` (a função exige um *trait* que o backend nativo não implementa) e o `jk build` traduz a mensagem para o código do Joker, com a sugestão (ref/08 §2.11.5). E contas em `f64`:

```rust
    #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f64>,
```

```
aviso[J0721]: `f64` num MCU sem FPU de dupla precisão
  --> micro/esp/apps/reflexo/src/lib.rs:9:74
   |
 9 |     #[param(unit = "ms", range = "0..2000", default = 300.0)] bloqueio: Param<f64>,
   |                                                                          ^^^^^^^^^^ a placa tem `fpu = "sp"`
   = nota: rotinas de vírgula flutuante por software chamadas por `reflexo::reflexo`: __adddf3, __ltdf2, __muldf3
   = ajuda: usa `f32`; mede o custo com `jk bench --target mcu:esp32-devkit`
```

O `J0720` recusa: a API não existe. O `J0721` só avisa: o `f64` funciona, mas é emulado. O ESP32 tem FPU de precisão simples, e o `jk build` procura no objeto da app as rotinas de dupla por software para te dizer quais são as funções culpadas. Num ESP32-C6, sem FPU nenhuma, até o `f32` daria o aviso `J0780` (documento 19, §3.1). A versão da secção 2 já é a corrigida.

## 5. Uma app é firmware

Uma app nativa é **código do firmware**. Por isso, acrescentar o `reflexo` não é só uma geração nova: é um firmware genérico novo (com a app lá dentro) e uma geração que a usa.

```sh
$ jk build --target mcu:esp32-devkit --generic
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
$ jk apply --env lab-link --node lab --confirm 5m
```

```
→ lab (dev) · de --node · lab.local:7443
plano da geração 14
  ~ MCU esp: firmware sha256:0be4… → sha256:61d9… (apps + reflexo 0.1.0) · 928 KiB pelo link · reinício
  ~ MCU esp: geração 4 → 5 · + app reflexo (supervisor:8 em io.led) · + grupo main: tarefa reflexo
  sem mudanças no nó
aplicado. confirmação pendente: `jk confirm` em 5 min, ou rollback automático
```

A geração refere a app por nome, major e `meta_hash`, e só é aceite por um firmware que a contenha (`J0772`). Depois do reinício de ativação e do `HELLO` com a geração 5 em experiência, `jk confirm --node lab` confirma o firmware e a geração.

## 6. Quem ganha, e porquê

Prime o `BOOT`. O LED do ESP32 muda **logo**, sem a viagem ao Pi 3. Pergunta quem manda nele:

```sh
$ jk explain esp.led --node lab
```

```
esp.led      bool · saída · alias de esp.io.led · MCU esp (joker-link, série)
  envelope   no MCU: safe = false                                   micro/esp/joker.toml
  grupo dono main 10ms, no MCU
  delegados  manual:3 manual:4 manual:5 control:12
  níveis     supervisor:8   true    app:reflexo · local no MCU · lease 30 ms            ← ativo
             control:12     true    app:luz · remoto (lab) · lease 100 ms
  aplicado   true   (decidido pelo árbitro do MCU; control:12 perde para supervisor:8)
```

```sh
$ jk read esp.led esp.button --watch --node lab
```

```
esp.led      true    good    (supervisor:8 · reflexo, no MCU)   luz: overridden(supervisor:8)
esp.button   false   good    idade 0,4 s
```

A `luz` continua a propor, como sempre: não mudou, e não sabe do reflexo. Mas a proposta dela perde, e o resultado que volta no `OUTCOMES` é `overridden(supervisor:8)`, registado no registo de decisões do nó (`jk decisions --output esp.led`). Como a regra é a mesma, as duas querem quase sempre o mesmo valor; quem o aplica é o reflexo.

Agora pára o `jokerd`:

```sh
lab$ sudo systemctl stop jokerd
```

Na parte 9, o LED ia para `safe` ao fim de 100 ms. **Agora não vai.** O lease que expira é o da `luz`, no nível `control:12`; o reflexo, em `supervisor:8`, renova o dele a cada tick local, e o LED continua a obedecer ao `BOOT`. O `safe` só se aplica quando não há **nenhum** nível ativo (A6), e há: um escritor que vive no mesmo chip que a saída. `lab$ sudo systemctl start jokerd` e a `luz` volta a propor, e a perder.

## 7. O link por Wi-Fi

O cabo série serviu bem, mas prende o ESP32 ao Pi 3. O ESP32 tem Wi-Fi, e o Joker Link corre em UDP (ref/07 §3.5). Três peças: a password da rede, o manifesto do MCU e o dispositivo no Pi 3.

**A password é um segredo.** Não vai para o Git nem para o manifesto em claro (`J0294`):

```sh
$ jk secret set wifi_bancada --nodes esp
```

```
valor de wifi_bancada: ••••••••••••
repete: ••••••••••••
wifi_bancada · rev 1 · cifrado para node:esp (x25519:3c8a…, registo 19, do enrolamento da parte 9)
aviso[W0926]: entrada sem assinatura `secrets` (em `prod` é um erro; parte 11)
✓ secrets.toml.enc
```

O valor fica cifrado para a chave X25519 do **MCU**, que nasceu no enrolamento e nunca saiu dele. O `jk build --generation` põe a entrada cifrada na secção `SECRETS` do blob (ref/08 §2.13.3), o nó `lab` leva o blob ao MCU pelo link, e o MCU decifra-a ao abrir a geração. **O Pi 3 transporta a password e não a consegue ler:** não é destinatário. O `secrets.toml.enc` fica na raiz do projeto, ao lado do `joker.lab-link.toml`, que é quem leva a geração do MCU ao nó; pode ir para o Git (ref/02 §2.4).

**O manifesto do MCU.** Tira o `[buses.uart0]` e troca o `[link]`:

```toml
[net.wifi]
ssid     = "bancada"
password = "secret:wifi_bancada"
country  = "PT"

[link]
address = "udp:192.168.1.20:7471"   # o nó lab (o IP do Pi 3): para onde o MCU envia
peer    = "lab"
auth    = "required"
```

**O Pi 3.** No `joker.lab-link.toml`, o dispositivo deixa o barramento e passa a escutar em UDP:

```toml
[devices.esp]
bus     = { unset = true }
address = "udp:192.168.1.57:7471"   # o MCU: o IP do ESP32 e a porta do link
report  = "50ms"
refresh = "5s"

[apps.luz]
writer = { lease = "1s" }
```

Os dois `address` apontam um para o outro: do lado Linux, o `address` do dispositivo é o IP e a porta **do MCU**; no manifesto do MCU, o `[link] address` é o **do nó** (ref/07 §15; ref/01 §5.22). São sempre IPs: reserva os dois no DHCP (o ESP32 mantém o MAC, e por isso o `192.168.1.57` que já tinha com o firmware de bancada; o do `lab` vê-se com `hostname -I`). A sessão identifica-se pelo `sid` do cabeçalho cifrado, e não pelo endereço de origem (ref/07 §3.5).

A chave é a mesma do link série (`secret:link.esp`): o link mudou de transporte, não de identidade. E a cifra é obrigatória. Experimenta tirá-la:

```
erro[J0179]: Joker Link sem autenticação num transporte IP
 --> joker.lab-link.toml:9:1
  |
9 | auth    = "none"
  | ^^^^^^^^^^^^^^^^ o dispositivo esp usa udp:192.168.1.57:7471
  = nota: em UDP e TCP a autenticação é obrigatória, e configurada nos dois lados sem negociação (ref/07 §4.6)
  = ajuda: `auth = "required"` (a omissão); num cabo série dentro de um quadro, `none` seria o aviso J0738
```

Sem negociação, um atacante na rede não consegue "baixar" a segurança de um link que a exige: os dois lados recusam tramas em claro (`J-E2104`).

**Porquê um lease de 1 s.** Com os 100 ms da parte 9, o `jk check` avisa:

```
aviso[W0730]: condição de vivacidade não cumprida: o lease de um escritor remoto não aguenta uma renovação perdida
  = nota: luz → esp.led por UDP (Wi-Fi): 2 × 20 ms + 10 ms (grupo main do MCU) + 0,2 ms (bloco de 1024 B)
          + 100 ms (latência de pico em Wi-Fi, ref/07 §11.4) + 1 ms = 151 ms > 100 ms
  = ajuda: por Wi-Fi, leases de escritores remotos ≥ 1 s, e nunca controlo rápido; em `prod` isto é um erro
```

Com 1 s: `2 × 320 ms + 10 + 0,2 + 100 + 1 = 751 ms ≤ 1000 ms`. O preço está na tabela da secção 8.

**Aplicar, pela última vez pelo cabo.** A geração nova chega ao MCU pelo link série, e é ela que muda o link para UDP:

```sh
$ (cd micro/esp && jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw)
$ jk apply --env lab-link --node lab --confirm 5m
```

O MCU troca de geração, liga-se ao Wi-Fi, e oferece o `HELLO` por UDP; o nó, já com a geração nova, responde. Se a password estiver errada, o MCU nunca chega ao nó, ninguém confirma, e **passados 5 min o MCU volta sozinho à geração com o link série**, e o nó à configuração anterior: a janela de confirmação é a rede de segurança de uma mudança de rede (ADR 0015). Com o `jk status` a mostrar a sessão `full` por UDP:

```
esp   joker-link   up · sessão full · Joker Link 1.0 · udp 192.168.1.57:7471 · cifrado (kid 9a1f…)
      geração 6 (flash, gen_b, confirming) · relógio holdover (TIME, erro 2,1 ms > time_tolerance 1 ms)
      tramas rx 2 410 · perdidas 0,3 % · retransmissões 4 · RTT 6,8 ms (p99 38 ms) · esp.rssi −58 good
```

`jk confirm --node lab`. O `esp.rssi`, que na parte 9 era `Bad(NotConnected)` (o MCU não tinha rede), tem agora valor.

## 8. Série contra Wi-Fi

| | Série (parte 9) | Wi-Fi, UDP (agora) |
|---|---|---|
| RTT | 1,6 ms, estável | 3–15 ms, picos de 100+ ms |
| cifra | opcional (`J0738` sem ela) | obrigatória (`J0179`) |
| `keepalive` / `link_timeout` | 50 ms / 200 ms | 1 s / 5 s |
| `report` | 20 ms | 50 ms: menos tempo de antena, e o toque não precisa de mais |
| `refresh` e `max_age` dos sinais `esp.*` | 1 s → 3 s | 5 s → 15 s (3 × `refresh`; `J0739` abaixo de 2 ×) |
| lease da `luz` | 100 ms | 1 s |
| `jokerd` morto → o lease da `luz` expira no MCU | ≤ 110 ms | ≤ 1,01 s |
| `esp_offline` (`!good(esp.uptime)`, 5 s) | ~5,2 s | ~10 s: 5 s de `link_timeout` + 5 s de `delay_on` |
| relógio do MCU | `synced` (δ < 1 ms) | `holdover` com a tolerância por omissão; `time_tolerance = "5ms"` se precisares de `synced` |

A regra da ref/07 §11.4: por Wi-Fi, o nó Linux supervisiona e ajusta; o que tem de ser rápido fica no MCU. É exatamente o que o reflexo faz.

## 9. O Pi 3 desligado, de verdade

Com o link no ar, o ESP32 já não precisa do cabo de dados. Liga-o ao portátil, só para ter energia, e desliga o Pi:

```sh
lab$ sudo poweroff
```

Prime o `BOOT`: o LED liga e desliga como antes. O MCU continua a oferecer um `HELLO` com *backoff* até 5 s entre tentativas, e quando o Pi 3 voltar a sessão sobe sozinha.

Com o ESP32 no portátil, mede a app no próprio chip:

```sh
$ (cd micro/esp && jk bench --target mcu:esp32-devkit)
```

```
placa esp32-devkit@1 · por série (espflash) · contador de ciclos CCOUNT, 240 MHz · sem sonda
  reflexo.reflexo    10 000 execuções   p50 1,4 µs   p99 1,9 µs   máx 2,6 µs
  tick main (10 ms)  10 000 ticks       p50 11 µs    p99 14 µs    máx 19 µs    0,2 % do período
✓ [wcet] em micro/esp/joker.lock · firmware e geração do MCU repostos
```

O ESP32 original não tem JTAG sem um adaptador externo, por isso a medida é feita pelo contador de ciclos do próprio núcleo e relatada por série (documento 23, §5.1). Com a versão `f64` da secção 4, o `reflexo.reflexo` passa para perto de 3 µs: é isso que o `J0721` quer dizer. Volta a ligar o ESP32 e o Pi 3 como estavam.

## 10. O Joker Micro no desenho

O Módulo C fecha aqui. O documento 19 dá três padrões ao Joker Micro, e a bancada tocou em dois:

1. **Coprocessador de I/O de um nó Linux** (parte 9): o Pi pensa, o MCU segura as saídas com o seu envelope e o seu `safe`. Se o Linux morrer, a segurança fica. É a resposta ao LED `ACT` da parte 2, que ficava como estava.
2. **Lógica no MCU** (esta parte): o reflexo é uma app como as outras, com o mesmo SDK, o mesmo árbitro e o mesmo registo de decisões, só que a correr ao pé do hardware.
3. **Nó autónomo** (não feito aqui): um MCU M2 com rede pode ser o nó inteiro e falar diretamente com o Deck pelo perfil micro do protocolo do agente, em MQTT ou CoAP (ref/10 §2.15; ref/07 §14). O ESP32 tem o hardware para isso; o resto da trilha mantém-no atrás do Pi 3.

O que **não** está na v1: apps em Wasm no MCU. Uma app nativa é firmware, e mudá-la é um OTA de firmware com reinício (a secção 5). A 1.4 traz o `wasmi` para MCUs M2, com troca de apps sem reflash (o tipo `app` do OTA fica reservado para isso). ML no MCU também é 1.4. E o que esta bancada não prova: os alvos de referência (RP2350, ESP32-C6) só em QEMU, e o half-duplex e o CAN do link sem teste real (documento 23, §8).

## 11. O que aconteceu por baixo

- **O mesmo SDK, outro backend.** A macro `#[joker::app]` gera, com `backend-native`, o contrato `NativeApp` em vez dos exports WIT; a app lê a imagem do latch e escreve no buffer de propostas do grupo, pelo mesmo formato do Linux, sem a fronteira Wasm (ref/08 §2.11.5).
- **Escritores locais e remotos no mesmo árbitro.** O reflexo é um escritor da tabela `WRITERS` do blob; a `luz` é outro, (link, `control:12`). Os dois `jk check`, o do MCU (`J0210`) e o do nó (`J0731`), garantem que nunca disputam o mesmo nível (16 A5, A12).
- **Segredos para quem os usa.** A password vai cifrada para o MCU e viaja dentro do blob; quem transporta não lê (ref/02 §2.4.8; ref/08 §2.13.3).
- **UDP com a mesma sessão.** O mesmo L2 cifrado, um datagrama por trama, o CRC-32C na mesma, e temporizações de IP (ref/07 §3.5, §5.6).
- **Mudar de rede com rede de segurança.** A troca de transporte é uma geração do MCU como outra qualquer, com a janela de confirmação contada no MCU (ref/07 §9.4).

## 12. Exercícios

1. Liga o Pi 3 depois do exercício da secção 9 com o LED do ESP32 aceso. O LED do Pi (a instância `sinal`) e o do ESP32 concordam? Porquê? Quem é a fonte da verdade do `ligado`, e como o mudarias sem tocar na `luz`?
2. Faz `jk write esp.led false --ttl 1m --reason "…" --node lab` com o reflexo ativo. O que mostra o `jk explain esp.led`? E se tirares a faixa `manual` de `remote_levels`?
3. Muda o reflexo para `writer = { level = "safety:2" }`. O `jk check` aceita? Que argumento tens para não o fazer?
4. Põe `requires_clock = "synced"` no `[apps.reflexo]` e deixa o link em Wi-Fi com a tolerância por omissão. O que acontece ao LED, e que código explica porquê?

```sh
$ git add -A && git commit -qm "trilha(10): app reflexo no MCU; Joker Link por Wi-Fi (UDP, cifrado)"
```

**Estado no fim desta parte:** app `luz` **0.6, sem mudanças**; app de MCU **`reflexo` 0.1.0**. Ficheiros novos: `micro/esp/apps/reflexo/`, `micro/esp/joker.lock` (o `[wcet]` do chip). Mudaram: `secrets.toml.enc` (a entrada `wifi_bancada`), `micro/esp/joker.toml` (a app, `[net.wifi]`, `[link]` em UDP) e `joker.lab-link.toml` (o `esp` por UDP, `report` e `refresh`, lease de 1 s). O ESP32 corre o firmware com o `reflexo` e a geração 6, pelo Wi-Fi; o Pi 3 na geração 15 do `joker.lab-link.toml`, confirmada. Fim do Módulo C.

**Próximo:** [Parte 11 — Produção e frota: assinaturas, segredos, o Deck e dois nós](11-production-and-fleet.md).
