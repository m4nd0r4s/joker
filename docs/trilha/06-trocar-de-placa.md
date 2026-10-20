# Parte 6 — Trocar de placa: o mesmo projeto no Pi Zero W, e a placa que ainda não existe

**Objetivo:** pôr o projeto `bancada` a correr num Raspberry Pi Zero W sem mudar uma linha da app `luz`: separar nos overlays o que é das Raspberry Pi do que é do Pi 3, escrever o perfil de uma placa que o registo não conhece, e falar com o mesmo ESP32 por Wi-Fi em vez de USB.

**Vais aprender:** refatorar overlays sem mudar o manifesto efetivo · o hash do manifesto efetivo · o que é (e o que não é) um perfil de placa · `jk board probe`, `jk board check`, `jk board new` · níveis de suporte e quando se publica uma placa · recursos numa placa de um núcleo · Modbus RTU e Modbus TCP com o mesmo perfil · dois mestres no mesmo equipamento · um lock por manifesto e WCET por placa · `jk diff --against`.

**Antes de começar:** a parte 5 feita (app `luz` 0.4.1 no lab). O Pi Zero W com um cartão SD e uma fonte micro-USB. O ESP32 continua ligado ao Pi 3 (é daí que vem a alimentação) e está na mesma rede Wi-Fi de 2,4 GHz. Precisas do IP do ESP32: a consola série mostrou-o quando gravaste o firmware de bancada na parte 4 (ou vê-o na tabela DHCP do router, e reserva-o lá).

---

## 1. A regra: trocar de placa só mexe em manifestos

O Zero W é outra máquina: ARMv6 de 32 bits, **um** núcleo, 512 MB, só Wi-Fi e nenhum USB livre para o ESP32. Mas a lógica é a mesma: piscar o LED da placa segundo a temperatura do CPU, com o botão, o toque e o LED do ESP32. A app não sabe em que placa corre; o que muda é o que o manifesto diz sobre a placa e sobre o caminho até ao ESP32. No fim da parte, o `jk diff` vai mostrar exatamente isso.

## 2. Primeiro, arrumar: o que é das Raspberry Pi e o que é do Pi 3

O `joker.lab.toml` mistura duas coisas: o dispositivo `pi` (o LED `ACT` e a temperatura pelo `sysfs`), que existe em qualquer Raspberry Pi, e o que é só do Pi 3 (a placa, os núcleos reservados, o ESP32 na porta USB). O comum passa para um overlay novo:

```toml
# joker.rpi.toml — o que é comum às placas Raspberry Pi
extends = ["joker.toml"]

[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act_led  = { led = "ACT", direction = "output" }
cpu_temp = { thermal = "thermal_zone0" }
```

```toml
# joker.lab.toml — nó lab (Raspberry Pi 3 B+)
extends = ["joker.rpi.toml"]

[target]
board = "rpi3b-plus@1"

[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
bg = { cpus = [0, 1] }

[buses.esp32]
kind = "serial"
port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
baud = 115200
dtr  = "low"
rts  = "low"

[devices.esp]
driver  = "modbus-rtu"
bus     = "esp32"
unit_id = 1
profile = "local/bench-esp32@1"

[security.api]
listen = ["tcp:[::]:7443"]
```

Porque é que o `[devices.pi]` não vai para o `joker.toml`? Porque o `joker.toml` é comum ao simulador e ao hardware, e no `joker.sim.toml` o `pi` é `driver = "sim"`. A cadeia fica `joker.lab.toml → joker.rpi.toml → joker.toml`, e a linearização é `joker.toml, joker.rpi.toml, joker.lab.toml` (ref/01 §7.1).

Uma refatoração tem de ser provada. O hash do manifesto efetivo é calculado depois da fusão, com as omissões materializadas; partir um ficheiro em dois overlays **não o muda** (ref/01 §8.3):

```sh
$ jk diff --env lab --node lab
```

```
manifesto efetivo  joker.lab.toml → sha256:3f9a41c0…
geração ativa      lab · geração 6 · sha256:3f9a41c0…
sem diferenças
```

E `jk explain devices.pi.driver --env lab` diz de que camada vem cada chave (aqui, `joker.rpi.toml:5`). O `joker.lab.lock` também não muda (`jk lock --check --env lab` sai com 0): o lock é do manifesto de entrada, e o manifesto de entrada é o mesmo (ref/02 §2.1.1). O `joker.rpi.toml`, que só entra por `extends`, não tem lock.

## 3. O Zero, primeira tentativa

A forma mais natural de começar é copiar o nó que já funciona e mudar a placa:

```sh
$ cp joker.lab.toml joker.zero.toml       # e, no joker.zero.toml, board = "rpi-zero-w@1" em vez de "rpi3b-plus@1"
```

O `joker.zero.toml` fica igual ao `joker.lab.toml` do §2, com uma linha diferente na linha 5. De propósito: o `jk check` vai dizer, uma a uma, as coisas que eram do Pi 3.

```sh
$ jk check --env zero
```

```
error[J0172]: placa `rpi-zero-w` não encontrada
  --> joker.zero.toml:5:9
   |
 5 | board = "rpi-zero-w@1"
   |         ^^^^^^^^^^^^^^ procurada em boards/, em oci://hub.example.com/joker e nas embutidas
   = nota: placas conhecidas parecidas: rpi3b-plus
   = ajuda: se é uma placa nova, escreve o perfil a partir do próprio nó:
            jk board probe --node zero --name rpi-zero-w --out boards/rpi-zero-w.board.toml
nota: as verificações que dependem da placa (recursos, memória, admissão RT) ficam por fazer
```

O `jk check` para aqui de propósito: os passos seguintes do manifesto efetivo precisam da placa (ref/01 §8.1, passo 4). O `jk build --target rpi-zero-w` dá o erro equivalente do lado do build, `J0827`.

A placa `rpi-zero-w` não está no registo, e não é por esquecimento: o Zero W é ARMv6, que o Joker só suporta no nível `dev` (documento 01, §22; documento 23, §7). Vais escrevê-la tu. Para isso, o Zero tem de ser um nó.

## 4. Preparar o Zero

1. **Sistema:** Raspberry Pi OS Lite **32 bits**, gravado com o Raspberry Pi Imager, com SSH ligado, o Wi-Fi configurado e o nome `zero`.
2. **O `jokerd` para ARMv6.** O pacote chama-se `armhf`, mas cuidado: o `armhf` do Debian é ARMv7, e um binário ARMv7 morre no Zero com `Illegal instruction`. O pacote do Joker para o Raspberry Pi OS 32 bits é compilado para `arm-unknown-linux-gnueabihf` (ARMv6 com VFP2). Instala-o como na parte 2:

```sh
zero$ uname -m && dpkg --print-architecture
armv6l
armhf
zero$ sudo apt install ./jokerd_1.0.0_armhf.deb
zero$ sudo jokerd enroll-code
```

3. **Enrolar**, com a CA da parte 2:

```sh
$ jk node enroll ssh://pi@zero.local --ca ~/org-ca --name zero --allow-dev --code 7KQ2M4XD9TPB3HVA
```

```
→ zero · ssh://pi@zero.local
identidade  P-256 em ficheiro (sem TPM: J0933, aviso) · KEM X25519
certificado CN=zero · emitido por ~/org-ca · política allow_dev = true
cliente     [nodes.zero] address = "ssh://pi@zero.local"
```

O nó está vivo, sem geração, à espera, e o contexto `zero` usa o endereço de arranque, como o `lab` na parte 2. É o que o probe precisa.

## 5. Escrever a placa que não existe

### 5.1 O que é um perfil de placa

Um perfil de placa descreve **o computador**, e só o computador: a arquitetura, os núcleos, a memória, o kernel com que é suportada, o watchdog, o IO fixo que vem soldado na placa (o LED `ACT`, o sensor de temperatura do SoC) e o que o hardware faz sozinho quando o `jokerd` para, as portas e as interfaces com nomes estáveis, e uma sugestão de `[resources]` (ref/03 §2.3).

**Não** descreve os equipamentos: o ESP32 não aparece lá, nem o que estiver ligado aos pinos. Os equipamentos são dispositivos do manifesto, com perfis de dispositivo (parte 4). E não tem decisões do site: nem `safe` (ADR 0006), nem `poll`, nem envelopes. Uma placa responde a "o que é esta máquina?", e é por isso que o mesmo perfil serve qualquer projeto que corra num Zero W.

Quem o usa: o `jk check` (núcleos, memória para o `J0380`, periféricos referidos, `recommended`), o `jk build` (arquitetura e motor Wasm), o `jk doctor` (kernel, watchdog) e o lock, que lhe fixa a versão e o digest.

### 5.2 O rascunho

O `jk board probe` pergunta ao nó o inventário do hardware (`GET /v1/node/hardware`, só leituras) e escreve um rascunho. O que não consegue saber lendo o nó, marca-o com `# REVER`:

```sh
$ jk board probe --node zero --name rpi-zero-w --out boards/rpi-zero-w.board.toml
```

```
→ zero · inventário lido (só leituras)
cpu     1 × ARM1176JZF-S (armv6l) · cpufreq máx. 1 GHz
memória revisão 9000c1 → 512 MiB (o kernel vê 427 MiB)
kernel  6.6.51+rpt-rpi-v6 · sem PREEMPT_RT
sysfs   leds: ACT · thermal: thermal_zone0 (cpu-thermal) · watchdog0 (bcm2835-wdt, máx. 15 s)
gpio    gpiochip0 "pinctrl-bcm2835" (54 linhas)
tty     serial0 → ttyS0
net     wlan0
rascunho boards/rpi-zero-w.board.toml · 5 campos a rever
```

O nome segue a regra da parte 4: `<placa>.board.toml`, com o esquema de placa no editor e `J0803` se o conteúdo não começar por `[board]`. O manifesto escreve só `board = "rpi-zero-w@1"`, e a resolução acrescenta o sufixo e procura em `boards/` (ADR 0026).

O rascunho tem as mesmas tabelas do ficheiro final da §5.3, mais um `[serial]`, e cinco linhas com `# REVER`:

```toml
# boards/rpi-zero-w.board.toml — rascunho de `jk board probe --node zero` (as linhas a rever)
[board]
model       = "Raspberry Pi Zero W Rev 1.1"          # lido de /proc/device-tree/model
tier        = "dev"                                  # REVER: nível de suporte; armv6 só é aceite em "dev"
kernel      = "standard"                             # REVER: kernel alvo; o nó corre um kernel sem PREEMPT_RT
recommended = { rt = [0], io = [0], bg = [0] }       # REVER: com 1 CPU não há onde isolar o rt

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"
# REVER: fallback. O que faz este LED quando o jokerd para? O probe não o sabe.

[serial]
"uart0" = "/dev/serial0"                             # REVER: no Zero W é o mini UART (ttyS0)
```

### 5.3 Rever, campo a campo

| Campo | Decisão | Porquê |
|---|---|---|
| `tier = "dev"` | fica | ARMv6 só existe no nível `dev` (ref/03 §2.3.1): serve para desenvolver e nunca é alvo de produção. Não há CI com este hardware |
| `arch = "armv6"` | fica | é o que decide o motor Wasm e o binário; com outro `tier` seria `J0829` |
| `cpus = 1` | fica | é a base do `J0144` nos `[resources]` do manifesto |
| `memory = "512 MiB"` | fica | a placa tem 512 MiB; os 427 MiB visíveis dependem do `gpu_mem` do sistema, que o `jk doctor` compara com o nó |
| `model` | `"Raspberry Pi Zero W (v1.1)"` | é texto, para pessoas |
| `kernel = "standard"` | fica | não há um kernel PREEMPT_RT mantido para esta placa, e com um núcleo não haveria onde o isolar. Sem `boot_cmdline` |
| `recommended` | fica, tudo na CPU 0 | não há alternativa; o §6 volta a isto |
| `[watchdog]` | fica | confirma que nenhum outro serviço o abre (o `jokerd` tem de ser o único dono) |
| `[io.act_led]` sem `fallback` | fica sem | experimenta: com o LED aceso, `sudo systemctl stop jokerd` no Zero, e o LED fica aceso. Não há fallback de hardware. E mesmo que houvesse, só contaria pelo caminho `board.act_led`, e não pelo `pi.act_led` do `joker.rpi.toml` (ref/03 §2.3.2): é por isso que o `J0205` aparece em `prod`, como no Pi 3 (parte 2) |
| `[serial]` | sai | o mini UART do Zero W tem o relógio preso ao do núcleo e o UART bom é do Bluetooth; nada no projeto o usa, e uma placa só promete o que garante |

Tira os comentários `# REVER` e muda o `model`. O ficheiro final é o do documento 23 §7:

```toml
# boards/rpi-zero-w.board.toml
schema = 1

[board]
name        = "rpi-zero-w"
version     = "1.0.0"
arch        = "armv6"
tier        = "dev"
vendor      = "Raspberry Pi"
model       = "Raspberry Pi Zero W (v1.1)"
cpus        = 1
memory      = "512 MiB"
clock       = "1 GHz"
storage     = "sd"
kernel      = "standard"
recommended = { rt = [0], io = [0], bg = [0] }

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"

[net]
"wifi" = "wlan0"
```

### 5.4 Validar contra o nó

```sh
$ jk board check boards/rpi-zero-w.board.toml --node zero
```

```
✓ esquema     schema 1 · rpi-zero-w 1.0.0 · linux · armv6 · tier dev
✓ coerência   1 CPU; recommended em 0..0; armv6 com tier dev
✓ no nó zero  armv6l · 1 CPU · 512 MiB · kernel standard
              /dev/watchdog0 · pinctrl-bcm2835 · leds/ACT · thermal_zone0 · wlan0
```

**O que o `jk board check` diz** quando se começa pelo atalho errado. O `jk board new` copia uma placa existente, e o Pi 3 parece um bom ponto de partida:

```sh
$ jk board new rpi-zero-w --from rpi3b-plus
$ jk board check boards/rpi-zero-w.board.toml --node zero
```

```
error[J0829]: a placa diz `arch = "aarch64"` e `cpus = 4`; o nó zero é armv6l com 1 CPU
  --> boards/rpi-zero-w.board.toml:6:1
error[J0828]: recurso inexistente no nó zero: /dev/i2c-1 ([i2c] "i2c1"), /dev/spidev0.0 ([spi] "spi0"), eth0 ([net] "eth")
  --> boards/rpi-zero-w.board.toml:37:1
   = ajuda: para uma placa que ainda não existe, começa pelo que o nó diz: jk board probe --node zero
```

O `jk board new` serve para variantes de uma placa conhecida (outra revisão, outra memória). Para uma máquina diferente, o probe é mais honesto: parte do que existe, e só te pede o que ele não pode saber.

### 5.5 Quando se publica uma placa

Esta fica em `boards/`, e o lock fixa-a pelo digest do ficheiro (`source = "path:…"`, ref/02 §2.1.4). Publica-se uma placa quando outros projetos a vão usar e ela descreve um alvo suportado, nível `1` ou `2`, com as versões da ref/03 §2.3.6. **Uma placa de nível `dev` nunca se publica** (`J0848`): num registo, qualquer manifesto a resolveria como se fosse um alvo a sério.

## 6. Agora, os problemas reais

Com a placa no sítio, o `jk check` passa do passo 4 e mostra o que foi copiado do Pi 3 sem ser das Raspberry:

```sh
$ jk check --env zero --node zero
```

```
error[J0144]: CPU 3 não existe na placa rpi-zero-w (1 CPU: 0)
  --> joker.zero.toml:8:15
   |
 8 | rt = { cpus = [3] }
   |               ^^^ também io.cpus = [2] (linha 9) e bg.cpus = [0, 1] (linha 10)
   = ajuda: retira [resources]: a omissão é o `recommended` da placa
error[J0828]: recurso inexistente no nó zero: porta /dev/serial/by-id/usb-Silicon_Labs_CP2102_… ([buses.esp32])
  --> joker.zero.toml:14:8
   = nota: o ESP32 está ligado ao Pi 3, e não a este nó
aviso[J0352]: grupo `main` (20 ms) sem PREEMPT_RT: jitter não garantido
  = nota: rpi-zero-w é `kernel = "standard"`; é o esperado nesta placa (documento 23, §5.2)
nota: motor Wasm em armv6: Pulley (interpretador), a confirmar na Fase 1 (documento 23, §5.2)
```

Um a um:

- **`[resources]`** era do Pi 3, e é por isso que vive no `joker.lab.toml` e não no `joker.rpi.toml`: o Pi 3 tem quatro núcleos e reserva o 3 para o `rt`. No Zero, retira-se a secção. Sem ela, os recursos são o `recommended` da placa: tudo na CPU 0. Numa placa de um núcleo, o `rt` partilha a CPU com o `io` e o `bg`; não há isolamento a fazer, e o `jk check` di-lo numa nota.
- **`J0352`** é um aviso, e fica. Um grupo de 20 ms sem PREEMPT_RT funciona, mas o jitter não tem garantia. É para isto que o Zero está na bancada: provar que o `jokerd` se degrada com avisos e não parte.
- **O motor Wasm.** No Pi 3, as apps são compiladas pelo Cranelift. Em ARMv6 não há Cranelift: o plano é o Pulley, o interpretador do wasmtime, se compilar para `arm-unknown-linux-gnueabihf`. Está marcado **a confirmar** (documento 23, §5.2). Se não compilar, o `jk check` dá o aviso `J0243` (a placa não tem motor Wasm para a arquitetura: só apps nativas, documento 04, §5), e esta parte fica a meio. A trilha assume que compila.
- **`[buses.esp32]`**: o ESP32 não está ligado ao Zero. Mas o firmware de bancada serve a mesma tabela de registos por **Modbus TCP**, no Wi-Fi, porta 502 (documento 23, §4).

O perfil é **o mesmo**. `driver = "modbus"` no perfil é a família, e a família `modbus` serve os dois transportes (ref/03 §2.2.2); os endereços `at`/`fc` não mudam. Muda só o dispositivo, e o barramento série desaparece:

```toml
# joker.zero.toml — nó zero (Raspberry Pi Zero W): o ESP32 por Wi-Fi
extends = ["joker.rpi.toml"]

[target]
board = "rpi-zero-w@1"

[devices.esp]
driver  = "modbus-tcp"
address = "192.168.1.57:502"
unit_id = 1
profile = "local/bench-esp32@1"

[security.api]
listen = ["tcp:[::]:7443"]
```

Antes de aplicar, confirma que o ESP32 responde por Wi-Fi, a partir do próprio Zero:

```sh
$ jk profile test local/bench-esp32@1 --uri "modbus-tcp://192.168.1.57:502?unit=1" --node zero
```

```
sinal    registo   cru    valor    qualidade   latência
led_rb   coil 0    1      true     good        38 ms
button   di 0      0      false    good        (mesmo pedido)
touch    in 0      61     61       good        41 ms
rssi     in 1      -58    -58      good        (mesmo pedido)
uptime   in 2–3    5214   5214 s   good        (mesmo pedido)
fw       hr 0      1      1        good        35 ms
3 pedidos · 7 sinais
```

O Modbus TCP não tem autenticação: qualquer máquina da rede pode escrever na coil 0. Na bancada isso é aceitável; num site, o Modbus TCP vive numa rede de campo separada.

## 7. Dois mestres no mesmo equipamento

O `led_rb` a `true` na tabela acima é o lab: o Pi 3 continua a falar com o ESP32 por RTU, e o firmware responde aos dois transportes ao mesmo tempo. **Não ligues os dois nós ao mesmo LED.**

- **Cada nó arbitra as suas saídas, e só as suas.** O árbitro, o envelope e o lease de `esp.led` no lab não sabem que o zero existe. Os dois escreveriam na coil 0 cada um o seu `ligado`, e o LED seguiria o último pedido a chegar. Cada nó veria, no readback, um valor que não escreveu.
- **O fallback do equipamento deixa de proteger.** O firmware apaga o LED ao fim de 2 s *sem pedidos de ninguém*. Com dois mestres, se o zero cair, os pedidos do lab mantêm o ESP32 "acompanhado", e o LED fica no último valor que o zero escreveu.
- Na v1 não há um árbitro entre nós: um sinal pertence a um nó. Um espaço de sinais do site partilhado por vários nós é a malha da v2 (documento 21, §8).

Por isso, antes de aplicar no zero, tira o ESP32 ao lab. A forma limpa é voltar a uma geração do lab que não tinha o `esp`: o `jk generations --node lab` mostra-a, a 3, da parte 3 (`luz` 0.2.0).

```sh
$ jk rollback 3 --node lab
```

O lab continua a piscar pela temperatura e não fala com o ESP32, que continua alimentado pela USB do Pi 3. A parte 7 repõe-no.

## 8. Lock, WCET e aplicar

```sh
$ jk build --target rpi-zero-w
$ jk lock --env zero --node zero
$ jk bench --env zero --node zero
```

Um lock por manifesto de entrada (D8): `joker.zero.toml` dá `joker.zero.lock`, ao lado dele, com a placa, o perfil, a app, o fallback do ESP32 lido pelo Wi-Fi e o WCET medido **nesta** placa:

```
→ zero (dev) · de --node
luz.pisca  100 000 execuções · Pulley · p50 150 µs · p99,9 240 µs · máx. 610 µs
aviso[J0432]: WCET medido (240 µs) acima do declarado pela app (20 µs)
  = nota: o declarado é a estimativa do autor; a admissão RT usa o medido, do lock, para esta placa
admissão RT  main 20 ms · 240 µs · 1,2 % do período ✓
escrito      joker.zero.lock [wcet] "luz.pisca" (board = "rpi-zero-w")
```

```toml
# joker.zero.lock (excerto; [profiles] e [apps] iguais aos do joker.lab.lock)
version = 1
runtime = "1.0.0"
node = "zero"

[boards."rpi-zero-w@1"]
resolved = "1.0.0"
digest = "sha256:…"
source = "path:boards/rpi-zero-w.board.toml"

[fallback.esp]
outputs = ["esp.led"]
timeout = "2s"
value = false
configured = false
verified = "2026-09-25T15:02:11Z"

[discovered.esp]
firmware = "1"

[wcet]
"luz.pisca" = { p999 = "240us", max = "610us", samples = 100000, board = "rpi-zero-w", runtime = "1.0.0", app = "sha256:…", measured = "2026-09-25T15:10:40Z" }
```

No `joker.lab.lock`, a mesma tarefa tem `p999 = "7us"` e `board = "rpi3b-plus"`: a mesma app, umas 35 vezes mais lenta num núcleo ARMv6 interpretado. Uma entrada de WCET só vale para a placa, a app e a runtime em que foi medida (`J0431`, ref/02 §2.1.13). É por isso que o WCET vive no lock de cada nó e não na app, e a parte 11 vai precisar disto.

```sh
$ jk apply --env zero --node zero --confirm 5m
$ jk config set nodes.zero.address zero.local:7443     # confirma pelo caminho novo, como na parte 2
$ jk confirm --node zero
$ jk read pi.act_led 'esp.*' --watch --node zero
```

O LED `ACT` do Zero pisca segundo a temperatura do CPU do Zero; o `BOOT` do ESP32 liga e desliga; o toque mexe o ritmo; o LED do ESP32 mostra o estado. Tudo por Wi-Fi. Repete a falha da parte 5, agora vista do Zero: tira o cabo do ESP32 do Pi 3 (fica sem corrente e sem Wi-Fi) e vê o `esp_offline` no zero e o `ACT` a continuar a piscar.

## 9. O que mudou entre as duas placas

```sh
$ jk diff --env zero --against joker.lab.toml
```

```
manifesto efetivo  joker.zero.toml (sha256:9d0e77b2…)  contra  joker.lab.toml (sha256:3f9a41c0…)

 [target]
-board = "rpi3b-plus@1"
+board = "rpi-zero-w@1"

 [resources]
-rt = { cpus = [3] }
-io = { cpus = [2] }
-bg = { cpus = [0, 1] }
+rt = { cpus = [0] }          # omissão (recommended de rpi-zero-w)
+io = { cpus = [0] }          # omissão
+bg = { cpus = [0] }          # omissão

-[buses.esp32]
-kind = "serial"
-port = "/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0"
-baud = 115200
-dtr  = "low"
-rts  = "low"
-…                            # parity, stop, data_bits, turnaround, duplex (omissões)

 [devices.esp]
-driver  = "modbus-rtu"
-bus     = "esp32"
+driver  = "modbus-tcp"
+address = "192.168.1.57:502"

iguais: [project], [devices.pi], [security.api], [signals] (2), [outputs] (2), [cycles.main],
        [calc] (2), [alarms] (2), [telemetry.record] (2), [apps.luz] (luz 0.4.1, os mesmos binds e params)
```

A diferença entre as duas placas é **placa, recursos e transporte**. A app, o perfil do equipamento, os alarmes, os `calc` e os cenários não mudaram. O `sim/scenarios/falha_esp.scenario.toml` continua a ser o teste dos dois nós, porque testa o site e não a placa.

## 10. O que aconteceu por baixo

| Nesta parte | Onde está |
|---|---|
| linearização e fusão dos overlays; `extends` relativo ao ficheiro que o declara | ref/01 §7.1, §7.2, §7.5 |
| o hash do manifesto efetivo não muda com a arrumação dos ficheiros | ref/01 §8.3 |
| perfil de placa: `[board]`, periféricos, IO fixo com `fallback`, `recommended`; resolução em `boards/`, no registo e nas embutidas | ref/03 §2.3, §3 ("Resolução") |
| `[target] board` desconhecida `J0172` (no build `J0827`); CPUs fora da placa `J0144`; recurso inexistente `J0828`; placa incoerente `J0829` | ref/01 §5.3, §5.4; ref/03 §2.9 |
| sem RT e sem núcleo livre: `J0352`, o comportamento degradado esperado | documento 23 §5.2; documento 01 §22 |
| família `modbus` com dois drivers (`modbus-rtu`, `modbus-tcp`) e o mesmo perfil | ref/03 §2.2.2; ref/01 anexo A |
| um árbitro por nó; o site com vários nós é a v2 | documento 16 §1; documento 21 §8 |
| um lock por manifesto de entrada, WCET por placa, fallback capturado pelo nó | ref/02 §2.1.1, §2.1.12, §2.1.13 |

## 11. Exercícios

1. Muda o `poll` do `esp` só no zero para `"200ms"`. Em que ficheiro? O `jk diff --against joker.lab.toml` mostra mais uma linha; o `joker.sim.toml` mudou?
2. No zero, corre `stress-ng --cpu 1 --timeout 2m` e vê `jk status --node zero`: o jitter do grupo `main` e os overruns. Compara com o `stress-ng --cpu 4` no Pi 3. O que é que o `J0352` avisava?
3. Escreve o `boards/rpi-zero-w.board.toml` numa versão `1.1.0` que acrescenta a `[i2c]` depois de ligares o I²C com `raspi-config`. É minor ou major? O `joker.zero.lock` muda sozinho?
4. Liga os dois nós ao ESP32 ao mesmo tempo, durante 30 s, e segue `jk decisions --output esp.led` nos dois. Quantas decisões vês, e quem manda no LED?

```sh
$ git add -A && git commit -qm "trilha(06): joker.rpi.toml, placa rpi-zero-w e o ESP32 por Modbus TCP no nó zero"
```

**Estado no fim desta parte:** app `luz` 0.4.1, sem mudanças, a correr no zero (geração 1, contexto `zero.local:7443`); o lab na geração 3 (a da parte 3, sem o `esp`). Novos: `joker.rpi.toml`, `joker.zero.toml`, `joker.zero.lock`, `boards/rpi-zero-w.board.toml`; `joker.lab.toml` passa a `extends = ["joker.rpi.toml"]`, sem mudar o manifesto efetivo. Um mestre de cada vez: a parte 7 começa por repor o lab.

**Próximo:** [Parte 7 — Classes: o mesmo sistema na camada de equipamentos](07-classes.md).
