# Parte 9 — O ESP32 passa a Joker Micro: o Joker Link e o `safe` no MCU

**Objetivo:** trocar o firmware de bancada do ESP32 pelo Joker Micro, ligá-lo ao Pi 3 pelo Joker Link no mesmo cabo USB, e passar o envelope e o `safe` do LED do ESP32 para dentro do próprio MCU, sem mudar uma linha da app `luz` 0.6.

**Vais aprender:** o manifesto de um Joker Micro · os drivers `gpio` e `esp32` num MCU · firmware genérico e blob de geração · `jk flash` por série · a chave do link (`jk node enroll --link`) · como o nó Linux nomeia os sinais de um MCU (catálogo, aliases) · `J0234` · níveis delegados ao link · OTA com A/B, janela de confirmação e retoma · a porta de saída mais importante da ADR 0020.

**Antes de começar:** partes 1–8 feitas (app `luz` 0.6, instâncias `led_esp`, `botao`, `toque` e `sinal`, o cenário `falha_esp` a passar). O nó `lab` enrolado e na última geração confirmada do `joker.lab.toml`. O ESP32 ligado ao Pi 3 por micro-USB, ainda com o firmware de bancada Modbus. O cabo micro-USB do ESP32 vai passar uma vez pelo portátil, para gravar e enrolar.

---

## 1. O que muda, e o que não muda

Até aqui o ESP32 era um **equipamento**: servia registos Modbus, e quem decidia o LED era o árbitro e o envelope do Pi 3. O `safe` do LED do ESP32 dependia do fallback do próprio firmware de bancada (apaga ao fim de 2 s sem pedidos), que o `jk lock` verificava contra o perfil.

A partir de agora o ESP32 corre **o mesmo núcleo** do `jokerd`, compilado `no_std` para Xtensa (documento 19, §2):

```
          parte 5                                        parte 9
 ┌─ Pi 3 (jokerd) ───────────────┐              ┌─ Pi 3 (jokerd) ──────────┐
 │ luz → árbitro → envelope ─────┼─ Modbus ─▶   │ luz ── PROPOSE (nível,   │
 │        (esp.led, safe=false)  │   coil 0     │         lease) ──────────┼─ Joker Link ─▶
 └───────────────────────────────┘              └──────────────────────────┘   (mesmo cabo)
                          ESP32: firmware de bancada          ESP32: Joker Micro
                          fallback fixo de 2 s                árbitro → envelope → safe=false
```

- **Muda:** o firmware do ESP32, o dispositivo `esp` no Pi 3 (de `modbus-rtu` para `joker-link`), e **quem manda** no LED do ESP32: o MCU. O `jokerd` passa a ser um escritor remoto, com nível e lease (documento 16, A12).
- **Não muda:** a `luz` 0.6, o `joker.toml`, as instâncias, o `joker.sim.toml` e os cenários. Os caminhos `esp.led`, `esp.button`, `esp.touch`, `esp.rssi` e `esp.uptime` continuam a existir no Pi 3 (secção 8). É o antes e depois mais claro da ADR 0020 (documento 23, §4).

## 2. A toolchain Xtensa

O ESP32 original é Xtensa LX6. O Rust para Xtensa não está no LLVM oficial: vem da Espressif, pelo `espup` (documento 23, §5.1).

```sh
$ cargo install espup espflash
$ espup install                      # toolchain "esp" + LLVM com Xtensa
$ . ~/export-esp.sh                  # põe-na no PATH desta shell (acrescenta ao teu perfil)
$ rustup toolchain list | grep esp
esp
```

O `espflash` já o usaste na parte 4 para gravar o firmware de bancada; agora é a `jk` que o chama, porque a placa o declara.

## 3. O manifesto do Joker Micro

O MCU tem o seu próprio manifesto, numa pasta à parte: `micro/esp/joker.toml`. É um `joker.toml` como os outros, validado pelo mesmo `jk check` e **compilado** para um blob, porque um MCU não interpreta TOML (ref/01 §5.22).

```toml
# micro/esp/joker.toml — o ESP32 como Joker Micro (parte 9)
[project]
name  = "bancada-esp"
joker = "1.0"
mode  = "dev"

[target]
board = "esp32-devkit@1"      # já existe no registo (documento 23, §7): não se escreve

[clock]
sync = "link"                 # o relógio vem das mensagens TIME do Joker Link

[buses.uart0]
kind = "serial"
port = "uart0"                # o UART0 da placa, pela ponte USB (gpio1/gpio3)
baud = 921600                 # o max_baud da placa

[link]
bus  = "uart0"
peer = "lab"                  # só aceita o nó lab
auth = "required"

# O LED e o botão: o driver gpio, com os pinos da placa
[devices.io]
driver = "gpio"

[devices.io.signals]
led    = { line = "gpio2", direction = "output" }
button = { line = "gpio0", bias = "pull-up", active_low = true, debounce = "20ms" }

# Toque, RSSI e tempo de funcionamento: o driver esp32 (só Joker Micro)
[devices.chip]
driver = "esp32"

[devices.chip.signals]
touch  = { touch = 0, unit = "1", deadband = 5 }   # T0 = GPIO4; o deadband passa a ser no MCU
rssi   = { rssi = true, unit = "1" }               # dBm; sem unidade no catálogo (logarítmica)
uptime = { uptime = true, unit = "s" }

[cycles.main]
period = "10ms"

# O envelope e o safe do LED vivem aqui, e já não no Pi 3
[outputs."io.led"]
safe          = false
group         = "main"
remote_levels = ["manual:3", "manual:4", "manual:5", "control:12"]
```

Linha a linha, o que é novo:

- **`[target] board`** aponta para um perfil de placa com `os = "none"`: `mcu_class = "m2"`, `fpu = "sp"`, `reset_safe = false`, as áreas de flash, o `[probe] tool = "espflash"` e as `[capacities]` (256 sinais, 64 saídas, 4 grupos). O `jk check` verifica a geração contra estas capacidades (`J0710`).
- **Dois drivers do MCU.** O `gpio` é o mesmo driver dos nós Linux; num MCU, `line` é o nome do pino da placa. O `esp32` só existe no Joker Micro, em placas ESP32: `touch = 0` é o pad T0, `rssi = true` e `uptime = true` são o rádio e o temporizador. O `deadband` do toque, que na parte 5 vinha do perfil e era aplicado no Pi, passa a ser aplicado na aquisição do MCU: uma variação abaixo de 5 nem chega ao fio.
- **`[outputs."io.led"]`**: o envelope, com o `safe`, é do MCU. O grupo `main` de 10 ms é o **grupo dono** da saída: é no latch dele que as propostas do Pi 3 entram e que os leases expiram.
- **`remote_levels`**: os níveis do árbitro do MCU que o link pode ocupar (16 A12, ref/07 §8.1). Por omissão seriam todos os que nenhum escritor local usa, isto é, os 16, incluindo `safety`. Aqui delegamos só o que o Pi 3 precisa: a faixa `manual` (para um `jk write`, que por omissão é `manual:4`) e o `control:12` da `luz`. Menos níveis delegados é menos coisas que um nó Linux pode fazer a este MCU.

## 4. O firmware genérico

Um firmware por cada configuração não escala (documento 19, §4.1). Por isso compila-se um **firmware genérico** por placa e conjunto de apps e drivers, e a configuração do site vai num **blob de geração** à parte, em flash.

```sh
$ cd micro/esp
$ jk build --target mcu:esp32-devkit --generic
```

```
a compilar joker-micro 1.0.0 para xtensa-esp32-none-elf (toolchain esp)
  drivers   gpio, esp32, extern                 (features: drv-gpio, chip-esp; chip esp32)
  apps      nenhuma
  link      uart, udp, tcp · link-crypto · gen-flash · ota · persist · trace
✓ target/joker/firmware.bin    912 KiB   (área firmware: 1536 KiB)
✓ target/joker/firmware.jkfw   descritor de firmware · sha256:7f30…
```

O `jk inspect target/joker/firmware.jkfw` mostra o descritor: placa e `board_id`, formato de blob e ISA, capacidades (com o hash delas), apps (nenhuma), drivers, transportes, `fpu = "sp"`, áreas de flash e `security_version`. É o **contrato** entre este firmware e os blobs que aceita: um blob feito para outras capacidades é recusado antes de gastar flash (ref/08 §2.14).

## 5. Gravar por série

O `jk flash` é local: fala com a placa pela porta série de quem o corre. Desliga o micro-USB do ESP32 do Pi 3 e liga-o ao portátil, como na parte 4.

```sh
$ jk flash
```

```
placa esp32-devkit@1 · [probe] tool = espflash, protocol = serial
porta /dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0 (detetada)
  bootloader  0x001000   esp-idf, 2 slots
  firmware    0x010000   912 KiB   ✓ verificado
  gen_a/gen_b vazias: o MCU arranca em modo de manutenção
```

Sem geração, o MCU está em **modo de manutenção** (ref/08 §2.12.3): pinos no estado de reset, nenhum sinal, e o link sobe com a configuração por omissão da placa (`uart0` a 115 200 baud), só para `HELLO`, `OTA`, tempo e catálogo. O LED do ESP32 fica como o reset o deixa.

## 6. A chave do link

O Joker Link é cifrado com ChaCha20-Poly1305 e uma chave por MCU, a `K_link`, que nasce no enrolamento (ref/07 §4.7). Com o ESP32 ainda no portátil:

```sh
$ jk node enroll esp --link esp --node lab --ca ~/org-ca
```

```
→ lab (dev) · de --node · lab.local:7443
MCU em manutenção na porta …CP2102…-port0 · placa esp32-devkit@1 · id de hardware 240ac45e913c
  K_link gerada (256 bits) · kid 9a1f3c07d2b84e61
  MCU: área IDENTITY, registo 16 (nome "esp") e registo 17 (chave do link "esp")
  MCU: registo 19 (chave X25519 de segredos) · certificado CN=esp da CA bancada, com essa chave no SAN
  secrets.toml.enc: + destinatário node:esp (ainda sem entradas; parte 10)
  nó lab: segredo secret:link.esp (cifrado para o nó)
enrolado. o nó lab reconhece o MCU "esp" no dispositivo joker-link "esp"
```

O primeiro `esp` é o **nome do MCU** (vai no `HELLO`); o `--link esp` é o **dispositivo** do nó Linux que o vai usar. O `--ca` é o que dá ao MCU uma chave X25519 atestada pela tua CA: sem ele, o link funcionava, mas nenhum segredo (a palavra-passe do Wi-Fi, na parte 10) poderia ser cifrado para o MCU (ref/02 §2.4.8). A chave nunca aparece em logs, na API nem no `jk doctor`; o `kid` pode aparecer (ref/07 §4.7). Volta a ligar o ESP32 ao Pi 3.

## 7. A geração do MCU

```sh
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
```

```
✓ micro/esp/joker.toml     2 dispositivos (gpio, esp32), 5 sinais, 1 saída, 1 grupo, 1 link
✓ capacidades              5/256 sinais · 1/64 saídas · 1/4 grupos (J0710: cabe)
aviso[W0786]: a placa esp32-devkit não tem saídas seguras no reset (`reset_safe = false`)
  = nota: até o firmware configurar os pinos, o GPIO2 é uma entrada com o pull-down interno de pino de arranque (o LED fica apagado), mas a placa não garante isso para os outros pinos; na bancada não há carga
aviso[W0788]: geração em flash não ligada a um nó
aviso[W0779]: blob de geração sem assinatura
  = ajuda: os três são erros em `mode = "prod"`; a parte 11 trata das assinaturas
✓ target/joker/bancada-esp.jkgen   6,1 KiB · gen 1 · geração sha256:5d21… · catálogo sha256:3b9e…
```

O blob tem as tabelas do núcleo tal como ele as usa em memória (registo de sinais, envelopes, árbitros, bytecode das expressões) e o **catálogo** do Joker Link: a lista do que o MCU exporta, com um hash (ref/08 §2.13; ref/07 §6.3). O MCU usa-o no sítio, sem o copiar.

## 8. Como ficam os caminhos no Pi 3

Esta é a decisão de desenho da parte. O nó Linux não escreve o perfil de um MCU: gera-o a partir do catálogo (`joker/link-3b9e…`, ref/03 §2.2.16), e o driver `joker-link` põe cada sinal exportado em **`<dispositivo>.<caminho no MCU>`** (ref/07 §8.3). No MCU, os caminhos são `io.led`, `io.button`, `chip.touch`, `chip.rssi` e `chip.uptime`, porque todo o sinal de um dispositivo tem o prefixo do dispositivo (ref/01 §3.8). Por isso, no Pi 3, os caminhos canónicos passam a ser `esp.io.led`, `esp.chip.touch`, e assim por diante.

Três formas de manter `esp.led`:

| Opção | Onde | Porque não, ou porque sim |
|---|---|---|
| aliases no MCU | `micro/esp/joker.toml` | não serve: o catálogo leva só o caminho canónico de cada sinal (chave 1.2), e os aliases do MCU não saem dele |
| mudar os `bind` e as instâncias para `esp.io.led` | `joker.lab-link.toml` | funciona, mas reescreve a `luz`, as instâncias e os alarmes num overlay, e o `joker.sim.toml` passava a ter outros caminhos |
| **aliases no nó Linux** | `joker.lab-link.toml`, em `[devices.esp.signals]` | **escolhida**: um alias é um segundo caminho para o mesmo `SignalId` (documento 16, P1), e é um ajuste permitido num sinal de perfil (ref/01 §5.8.1). O `joker.toml`, as apps e as instâncias continuam a usar `esp.led` |

`esp.led_rb` e `esp.fw` desaparecem, e não fazem falta: o valor **aplicado** vem na própria amostra de `esp.led` (quem aplica é o MCU), e a versão do firmware vem no `HELLO` (secção 10). Se algum manifesto os referisse, o `jk check` dava `J0101`, com a sugestão.

## 9. O `joker.lab-link.toml`

```toml
# joker.lab-link.toml — o ESP32 como Joker Micro pelo Joker Link (parte 9)
extends = ["joker.lab.toml"]

[buses.esp32]
baud = 921600                     # o mesmo cabo, mais depressa

[devices.esp]
driver  = "joker-link"
micro   = "micro/esp/joker.toml"  # o manifesto do MCU que este dispositivo corre
unit_id = { unset = true }
profile = { unset = true }        # o perfil é gerado do catálogo
auth    = "required"
node    = "esp"                   # o nome que o HELLO tem de trazer
report  = "20ms"
refresh = "1s"

[devices.esp.signals]
"io.led"      = { alias = "esp.led" }
"io.button"   = { alias = "esp.button" }
"chip.touch"  = { alias = "esp.touch" }
"chip.rssi"   = { alias = "esp.rssi" }
"chip.uptime" = { alias = "esp.uptime" }

[apps.luz]
writer = { lease = "100ms" }      # folga para uma renovação perdida pelo link (ver abaixo)
```

Falta de propósito uma coisa. Corre o `jk check`:

```sh
$ jk check --env lab-link
```

```
erro[J0234]: envelope num caminho que não é uma saída deste nó
 --> joker.toml:14:1
   |
14 | [outputs."esp.led"]
   | ^^^^^^^^^^^^^^^^^^^ `esp.led` é um alias de `esp.io.led`, uma saída do Joker Micro `esp`
   = nota: as saídas de um dispositivo joker-link são governadas pelo envelope do MCU
           (micro/esp/joker.toml:44, safe = false); o nó Linux não as arbitra (ref/07 §8.1)
   = ajuda: em joker.lab-link.toml, `[outputs]` com `"esp.led" = { unset = true }`
```

Dois envelopes para a mesma saída, um em cada lado do cabo, seriam duas verdades. O Joker recusa. Acrescenta ao `joker.lab-link.toml`:

```toml
[outputs]
"esp.led" = { unset = true }      # o envelope desta saída passa a viver no MCU
```

Não é um alargamento que peça `widen`: o envelope não desaparece, muda de dono, e o do MCU tem o mesmo `safe = false`.

```sh
$ jk check --env lab-link
```

```
✓ manifesto          2 dispositivos (sysfs, joker-link) · 1 barramento (serial 921600)
✓ esp (joker-link)   micro/esp/joker.toml · catálogo sha256:3b9e… · 5 sinais, 1 saída
✓ aliases            esp.led → esp.io.led · esp.button → esp.io.button · … (5)
✓ ligações           led_esp (std/led@1) → esp.led · escritor luz control:12 (delegado ao link)
✓ vivacidade         lease 100 ms ≥ 54,7 ms = 2 × 20 ms (renovação) + 10 ms (grupo main do MCU)
                     + 3,1 ms (bloco de 256 B a 921 600 baud) + 0,6 ms (PROPOSE) + 1 ms   (J0730)
✓ tráfego do link    D→H 7 % · H→D 4 %   (J0732 acima de 70 %)
```

Duas verificações que só existem porque há um link. **Níveis delegados:** a `luz` escreve em `control:12`, que o MCU delega; com a `luz` em `optimizer`, seria `J0731`. **A condição de vivacidade** (ref/07 §11.6): o lease tem de aguentar uma renovação perdida, o período do grupo dono no MCU, um bloco de OTA a meio no mesmo sentido e o tempo no fio. O lease por omissão de um escritor Linux numa saída de MCU é 3 × o período do grupo Linux do escritor (60 ms), limitado pelo `max_lease` do link (ref/07 §8.1): passava por pouco; com 100 ms há folga. O custo: o `safe` chega, no pior caso, 100 ms depois de o Pi 3 morrer. Também a instância `sinal`, do Pi, fica com leases de 100 ms, porque uma app tem um só escritor (ref/01 §5.12).

## 10. Aplicar: o nó leva o blob ao MCU

```sh
$ jk apply --env lab-link --node lab --confirm 5m
```

```
→ lab (dev) · de --node · lab.local:7443
plano da geração 10
  ~ barramento esp32: 115200 → 921600 baud
  ~ dispositivo esp: modbus-rtu → joker-link (auth required, kid 9a1f…, catálogo sha256:3b9e…)
  - envelope esp.led (passa para o MCU esp)
  ~ app luz: lease 60 ms → 100 ms (recarrega, sem reiniciar)
  + MCU esp: geração 1 (bancada-esp.jkgen, 6,1 KiB) pelo link · área gen_a · janela 5 min
aplicado. confirmação pendente: `jk confirm` em 5 min, ou rollback automático
```

O que acontece no cabo: o `jokerd` abre a porta a 921 600 baud; o MCU, em manutenção, oferece um `HELLO` cifrado com a `K_link`; o nó responde `maintenance` (não há geração válida); o nó envia o blob por `OTA`; o MCU verifica-o **antes** de apagar a área, grava-o em `gen_a`, ativa-o num limite de tick e fecha a sessão (`BYE generation_change`); novo `HELLO`, agora com geração, e o nó responde `full`.

```sh
$ jk status --node lab
```

```
dispositivos
  pi    sysfs        up
  esp   joker-link   up · sessão full · Joker Link 1.0 · série 921600 · cifrado (kid 9a1f…)
        MCU esp · esp32-devkit@1 · firmware 1.0.0 (slot A) · arranques 3 (último: power_on)
        geração 1 (flash, gen_a, confirming, faltam 4 min 31 s) · relógio synced (TIME, δ 0,4 ms)
        tramas rx 1 882 · tx 1 907 · COBS/CRC 2 · autenticação 0 · replay 0 · retransmissões 0
        RTT 1,6 ms · lease_expired 0
```

Os dois erros de enquadramento são o lixo do ROM de arranque do ESP32 a 115 200 baud, que o COBS descarta até ao primeiro `0x00` (documento 23, §2). Ficam em contadores separados dos de autenticação, e é essa separação que permite distinguir ruído de ataque (ref/07 §3.1). Os contadores do próprio MCU também são sinais: `jk read 'joker.device.esp.**'`.

O LED do ESP32 volta a mostrar `ligado`, e o `BOOT` liga e desliga como na parte 4. `jk confirm --node lab` confirma a geração 10 do nó e, pelo link, a geração 1 do MCU (`OTA confirm`).

Fixa no lock o que o `HELLO` disse, com `jk lock --env lab-link --node lab`. O `[mcu.esp]` guarda o alvo (`mcu:esp32-devkit`), o modo (`generation`), as versões do Joker Micro e do Joker Link, as features, as capacidades e o digest do firmware genérico; o `[discovered.esp]` guarda o id de hardware (`240ac45e913c`), a versão do firmware e, em `digest`, o hash do catálogo (ref/02 §2.1.10–§2.1.11). O lock nunca fixa o digest da geração compilada, que é função do próprio lock. O `joker.lab-link.lock` já não tem o perfil `local/bench-esp32@1` nem o `[fallback.esp]`: esses continuam no `joker.lab.lock`, porque cada manifesto de entrada tem o seu lock. Em `prod`, um MCU que não bata com isto fica em `observe`: o nó lê, não escreve, e dispara o alarme de deriva (ref/07 §7.3, `J-E2202`).

## 11. As provas

**(a) Matar o `jokerd`.** Com o sistema ligado (o LED do ESP32 aceso):

```sh
lab$ sudo systemctl kill -s KILL jokerd
```

O LED do ESP32 **apaga-se em cerca de 100 ms**. Ninguém lhe disse nada: o último `PROPOSE` da `luz` foi consumido pelo MCU num tick, o lease de 100 ms contou a partir desse tick (A2: a latência do link não o come), expirou no latch do grupo `main` do MCU, e o árbitro do MCU, sem nenhum nível ativo, aplicou o `safe` (A6). O link só é dado como perdido 200 ms depois (`link_timeout`), e isso não interfere com a saída (ref/07 §7.4).

Compara com a parte 2 (c): lá, o LED `ACT` ficava aceso, porque o `sysfs` não tem fallback. E com a parte 5: lá, o firmware de bancada apagava o LED ao fim de 2 s, por uma regra fixa no firmware que o Joker só podia verificar. **Aqui o `safe` é o próprio desenho**, e é a mesma regra que protege uma saída no Linux. É a porta de saída mais importante da ADR 0020 (documento 23, §5.3).

O systemd reinicia o `jokerd` em 1–2 s. O nó envia `HELLO(pedido)`, o MCU obedece depois de 2 × `keepalive` em silêncio, a sessão sobe em poucos milissegundos, e a `luz` volta a propor. O `jk status` mostra `arranques 3`: o MCU não reiniciou, só a sessão é nova. O LED volta a acender: a `luz` arrancou de novo, com o estado inicial, `ligado = true`.

**(b) Uma geração nova por OTA.** Muda o `deadband` do toque no MCU de 5 para 3 e volta a construir e a aplicar:

```sh
$ (cd micro/esp && jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw)
$ jk apply --env lab-link --node lab --confirm 5m
```

```
plano da geração 11
  ~ MCU esp: geração 1 → 2 (sha256:5d21… → sha256:a07c…) · 6,1 KiB pelo link · área gen_b
      ~ chip.touch.deadband 5 → 3
    troca a quente num limite de tick; janela de confirmação 5 min, contada no MCU
  sem mudanças no nó
```

O blob novo vai para a **outra** área (`gen_b`), enquanto o MCU continua a correr a geração 1 em `gen_a`. Verificado, troca num limite de tick: o LED mantém o valor aplicado, os leases e os trips (A8), e o `HELLO` seguinte mostra a geração 2 em experiência. `jk confirm --node lab` confirma as duas.

Agora repete com `deadband = 8` e **não confirmes**. Passados 5 min:

```
esp   … geração 3 rolled_back → geração 2 (gen_b) · J-E2409: a atualização não foi confirmada na janela
```

O MCU voltou sozinho, sem ninguém lhe dizer nada. A janela conta **no MCU**, porque o nó Linux pode ser a causa do problema (ref/07 §9.4; ADR 0015). O nó também volta à geração 11. Repõe o `deadband = 3` no ficheiro.

**(c) Desligar o cabo a meio de um OTA.** O blob é pequeno demais para o apanhar a meio (menos de 0,1 s), mas o firmware não é. E há uma razão boa para um firmware novo: o anel de traço de 16 KiB guarda poucos segundos, e queres o dobro. As capacidades são do **firmware**, e não da geração: acrescenta ao `micro/esp/joker.toml` uma tabela `[target.capacity]` com `trace_bytes = "32 KiB"`, e reconstrói os dois, pela ordem certa:

```sh
$ cd micro/esp && jk build --target mcu:esp32-devkit --generic
$ jk build --target mcu:esp32-devkit --generation --firmware target/joker/firmware.jkfw && cd ../..
$ jk apply --env lab-link --node lab --confirm 5m
```

O blob novo leva o hash das capacidades do firmware novo; enviado ao firmware antigo, seria recusado com `J-E2410`. O plano mostra primeiro o firmware (912 KiB, com reinício) e depois a geração. Quando a barra passar dos 40 %, **desliga o micro-USB do ESP32**:

```
MCU esp: OTA firmware   ███████░░░░░░░░░░░   372 / 912 KiB
MCU esp: link perdido (J-E2205) · a transferência fica guardada no MCU
MCU esp: à espera do HELLO …
                        (o ESP32 ficou sem energia: é o mesmo cabo. Volta a ligá-lo.)
MCU esp: sessão nova · arranques 3 → 4 (power_on) · firmware 1.0.0 (slot A), inalterado
MCU esp: OTA begin com o mesmo manifesto → ready em 376 832 B (último setor completo)
MCU esp: OTA firmware   ██████████████████   912 / 912 KiB · verified
MCU esp: activate → reinício (BYE ota_activate) · firmware novo em experiência (slot B)
MCU esp: OTA geração 4 → gen_a · activate · sessão full
```

O MCU guardou em `PERSIST` o par (digest, offset confirmado), arredondado ao último setor, e o `begin` com o mesmo manifesto retomou dali (ref/07 §9.3). Enquanto o firmware antigo corria sem link, o LED estava em `safe`; durante o reinício de ativação, esteve no estado de reset do hardware, que nesta placa não é garantido (o `W0786` da secção 7). O firmware novo só se marca como bom depois de ticks limpos e do `HELLO`; `jk confirm --node lab` fecha as duas experiências.

## 12. Parte 5 contra parte 9

| | Parte 5 (Modbus RTU) | Parte 9 (Joker Link) |
|---|---|---|
| quem arbitra e aplica o envelope do LED | o `jokerd` no Pi 3 | o MCU |
| `safe` quando o Pi morre | fallback fixo do firmware, 2 s, verificado pelo `jk lock` (`J0205`) | o `safe` do envelope do MCU, no fim do lease (≈ 100 ms) |
| leituras | *polling* a 100 ms, o Pi pergunta | o MCU envia as alterações (`report` 20 ms) e um `refresh` a cada 1 s |
| qualidade | feita no Pi (timeout, CRC) | feita no MCU, e transportada tal e qual; `Bad(CommFailure)` no Pi se o link cair |
| confirmação da escrita | `led_rb` (coil lido de volta) | o valor aplicado na amostra de `esp.led` e o `OUTCOMES` |
| segurança no cabo | nenhuma | ChaCha20-Poly1305, anti-replay, `K_link` por MCU |
| mudar o comportamento do ESP32 | regravar o firmware à mão | uma geração por OTA, com A/B e confirmação |

E no simulador nada mudou, porque o `joker.sim.toml` estende o `joker.toml`, e não o `joker.lab-link.toml`: `jk test --env sim sim/scenarios/falha_esp.scenario.toml` continua a dar `ok` com a `luz` 0.6.0. O comportamento visto de fora é o mesmo desde a parte 5; mudou quem o garante.

## 13. O que aconteceu por baixo

- **Um núcleo, dois hospedeiros.** O árbitro e o envelope do MCU são o mesmo `joker-core` do `jokerd`, `no_std`, sem alocação no tick, e com a mesma bateria de conformidade, byte a byte (documento 19, §2; ADR 0020).
- **O firmware diz o que sabe fazer, a geração diz o que fazer.** O descritor `.jkfw` e o blob `.jkgen`, verificado no arranque e em cada OTA: formato, integridade, capacidades, anti-rollback (ref/08 §2.12–§2.14).
- **O `HELLO` decide o modo** (`maintenance`, `observe`, `full`), e o catálogo viaja por hash (ref/07 §6.3, §7.3).
- **Escritores remotos.** Cada bloco de um `PROPOSE` é um escritor (link, nível) do MCU; o lease conta no tick do MCU que o consome; os resultados voltam num `OUTCOMES` fiável, porque são decisões (16 A12; ref/07 §8).
- **OTA sem medo.** Manifesto COSE antes da imagem, retoma por offset persistido, confirmação contada no MCU (ref/07 §9).

## 14. Exercícios

1. Muda a `luz` para `writer = { level = "optimizer", lease = "100ms" }` no `joker.lab-link.toml`. Que código dá o `jk check`, e porquê? Onde se corrige: no Pi ou no MCU?
2. Baixa o lease da `luz` para `"40ms"`. Lê o `J0730` e refaz a conta da secção 9 à mão. O que acontece à conta se subires o `bulk_max` do link para 1024?
3. Faz um `jk write esp.led true --ttl 30s --reason "…" --node lab` e mata o `jokerd` a meio. Quanto tempo fica o LED aceso? Relaciona a resposta com o `max_lease` do link (ref/07 §7.4, A10).
4. Tira a faixa `manual` de `remote_levels` e repete o `jk write`. Que resultado aparece, e onde o vês?

```sh
$ git add -A && git commit -qm "trilha(09): o ESP32 como Joker Micro pelo Joker Link; safe no MCU"
```

**Estado no fim desta parte:** app `luz` **0.6, sem mudanças**. Ficheiros novos: `micro/esp/joker.toml`, `joker.lab-link.toml`, `joker.lab-link.lock`, e `secrets.toml.enc`, por agora só com o destinatário `node:esp`. O `micro/esp/target/` fica fora do Git. Fora do projeto: a toolchain `esp`; o ESP32 com o Joker Micro (firmware com traço de 32 KiB, geração 4) e enrolado no nó `lab` (`secret:link.esp`); o nó `lab` na geração 13 (a do `joker.lab-link.toml` com o firmware novo), confirmada.

**Próximo:** [Parte 10 — Lógica no MCU: uma app nativa e o link por Wi-Fi](10-logica-no-mcu.md).
