# Parte 2 — O Pi 3 como nó: instalar, enrolar, a placa, o LED real

**Objetivo:** transformar o Raspberry Pi 3 B+ num nó Joker (`lab`) e pôr **a mesma app, sem mudar uma linha**, a piscar o LED `ACT` real da placa. Depois, ver o árbitro a decidir entre ti e a app, e descobrir o que acontece quando as coisas correm mal.

**Vais aprender:** instalar o `jokerd` · a CA da organização e o enrolamento sem portas abertas · `jk doctor --rt` · o perfil de placa e `[resources]` · o driver `sysfs` · um lock por manifesto · `jk apply --confirm`, gerações e rollback · escrita manual e árbitro · o estado seguro em três camadas, e o que ele não cobre.

**Antes de começar:** a parte 1 feita (app `luz` 0.1). O Pi 3 B+ com um cartão microSD e a fonte, na mesma rede Wi-Fi que o portátil. Nenhum fio além da alimentação.

---

## 1. Preparar o Pi 3

Com o Raspberry Pi Imager, grava o **Raspberry Pi OS Lite (64 bits)**. Nas opções do Imager: nome de host `lab`, utilizador `pi`, a tua rede Wi-Fi, SSH com a tua chave pública. Arranca o Pi e confirma:

```sh
$ ssh pi@lab.local uname -m
aarch64
```

## 2. Instalar o `jokerd`

```sh
lab$ sudo apt install ./jokerd_1.0.0_arm64.deb
lab$ sudo usermod -aG joker-admin pi          # para poderes enrolar o nó por SSH (secção 4)
lab$ exit                                     # volta a entrar para o grupo novo contar
```

O pacote cria o utilizador `joker` (com acesso aos grupos dos dispositivos), os grupos `joker-admin` e `joker-ops`, a unidade systemd, a regra udev que dá ao grupo `joker` escrita nos LEDs de `/sys/class/leds` e um `/etc/joker/node.toml` só com `version = 1`.

```sh
lab$ systemctl status jokerd
```

```
● jokerd.service - Joker runtime
     Active: active (running)
jokerd[612]: aviso[J0902]: nó sem identidade: em modo de enrolamento
jokerd[612]: a escutar só em unix:/run/joker/api.sock (sem identidade não há TCP)
```

Sem identidade, o `jokerd` escuta **só** o socket Unix, mesmo que lhe peças TCP: não há nenhuma porta de enrolamento aberta à rede (ref/02 §3.2.1).

## 3. A CA da organização

No portátil, cria a CA que vai assinar os certificados dos nós e o teu. Uma vez, e fora do projeto:

```sh
$ jk ca init ~/org-ca --org bancada
```

```
criado ~/org-ca/root.key        raiz Ed25519 (assina bundles de confiança; guarda-a bem)
criado ~/org-ca/ca.key, ca.crt  emissora P-256 (assina certificados de nós e de utilizadores)
criado ~/org-ca/org             bancada
certificado de utilizador       admin-dl → ~/.local/share/joker/certs/admin-dl.{crt,key}
registado em [user] de ~/.config/joker/config.toml
```

A raiz e a emissora são chaves diferentes de propósito: a emissora assina todos os dias; a raiz só assina a política de confiança (ref/10 §3.1.1; ADR 0025). Na parte 11 a raiz vai assinar mais coisas.

## 4. Enrolar o nó

```sh
$ jk node enroll ssh://pi@lab.local --ca ~/org-ca --name lab --allow-dev
```

```
→ ssh://pi@lab.local · socket /run/joker/api.sock · principal unix:pi (joker-admin)
CSR recebido          lab · chave P-256 em ficheiro (sem TPM) · KEM x25519
certificado emitido   CN=lab · SAN DNS:lab · CA bancada (~/org-ca)
confiança             âncora ed25519:5e0c… (raiz bancada) · bundle sem Deck
política              allow_dev = true (--allow-dev)
cliente               [nodes.lab] acrescentado a ~/.config/joker/config.toml
nó enrolado. escutas: unix:/run/joker/api.sock
```

O que aconteceu, pela ordem da ref/02 §3.2.3:

1. **O canal de arranque é o socket local, por SSH.** A `jk` abriu um túnel SSH até `/run/joker/api.sock`. Quem enrola prova que tem acesso ao nó por ser do grupo `joker-admin` (ou, sem isso, com o código de uso único do `jokerd enroll-code`, `--code`). Um endereço TCP não serve: `J-E1912`.
2. O `jokerd` gerou as chaves no próprio nó (a de identidade e a KEM, para os segredos da parte 11). **A chave privada nunca saiu do Pi.** Não há TPM no Pi 3, e por isso a chave fica em ficheiro; o `jk doctor` di-lo (`J0933`).
3. A `jk` assinou o CSR com a emissora de `~/org-ca` e instalou o certificado e o primeiro bundle de confiança.
4. O `jokerd` escreveu o `node.toml`. **`--allow-dev` é obrigatório aqui:** a CA de `jk ca init` não é de desenvolvimento, e sem ele o nó recusaria as gerações `mode = "dev"` do projeto com `J0906`.
5. A `jk` acrescentou o contexto `lab` à tua configuração.

```toml
# ~/.config/joker/config.toml
version = 1

[user]
name = "dl"
cert = "~/.local/share/joker/certs/admin-dl.crt"
key  = "~/.local/share/joker/certs/admin-dl.key"

[defaults]
search = ["oci://hub.example.com/joker"]

[nodes.lab]
address = "ssh://pi@lab.local"
ca      = "~/org-ca/ca.crt"
```

O `address` do contexto é o **endereço de arranque**, `ssh://…`: a `jk` não adivinha uma escuta TCP que o nó ainda não tem (ref/02 §3.2.3, passo 5). **A escuta TCP com mTLS não vem do enrolamento: vem de uma geração** (secção 7), porque escutar na rede é política do site, e o `node.toml` só guarda o mínimo que tem de sobreviver a uma geração má (ref/02 §2.2.1). Tudo o que segue funciona já pelo SSH.

## 5. `jk doctor --rt`

```sh
$ jk doctor --rt --node lab
```

```
→ lab · sem geração · de --node
placa            Raspberry Pi 3 Model B Plus Rev 1.3 → rpi3b-plus@1 (nível 2)
kernel.preempt_rt    não     6.12.25-v8 (PREEMPT_DYNAMIC)
   aviso[J0352]: grupos ≥ 10 ms correm sem jitter garantido; < 10 ms serão recusados (J0351)
cpu.isolcpus         —       a placa sugere: isolcpus=3 nohz_full=3 rcu_nocbs=3
cpu.governor         ondemand   sugestão: performance
watchdog             /dev/watchdog0 · 15 s · ok
clock                NTP sincronizado (systemd-timesyncd)
permissions, groups  ok
disk                 22 GiB livres
aviso[J0933]: chave de identidade em ficheiro, sem TPM nem elemento seguro
rt.cyclictest        60 s em cpu 3 · p99 118 µs · máx 2,4 ms   (registado; sem alvo nesta placa)
device_fallbacks     —   (nenhuma geração)
```

- **PREEMPT_RT.** O kernel do Raspberry Pi OS não tem `PREEMPT_RT`. O código de preempção RT está no kernel oficial desde a 6.12, mas é uma opção de compilação, e este kernel vem sem ela. Consequência: o grupo `main`, de 20 ms, corre com o aviso `J0352`, e um grupo abaixo de 10 ms seria recusado (`J0351`). Para piscar um LED e ler uma temperatura, 20 ms sem RT chegam. Compilar um kernel RT é um exercício opcional, e as medições com e sem ele ficam registadas na bancada (documento 23, §5.2).
- **`isolcpus=3`.** Isto já vale a pena: tira o núcleo 3 ao escalonador geral, para o grupo `rt` o ter só para si. Acrescenta `isolcpus=3 nohz_full=3 rcu_nocbs=3` ao fim da linha única de `/boot/firmware/cmdline.txt` e reinicia (`lab$ sudo reboot`). O `jk doctor --rt` passa a mostrar `cpu.isolcpus  3 · ok`.
- **Números sem alvo.** Os alvos de jitter do documento 01, §21 são para CM4/CM5. No Pi 3 ficam registados como referência pessimista: a Ethernet e o Wi-Fi passam pelo USB e geram interrupções.

Não declares `kernel = "preempt_rt"` no manifesto enquanto o nó não o tiver: o manifesto diz o que o nó tem, e o `jk doctor` verifica-o.

## 6. A placa

Um **perfil de placa** descreve o hardware fixo de um modelo de computador: CPUs, memória, kernel suportado, watchdog, IO fixo, portas e barramentos (ref/03 §2.3). O do Pi 3 B+ já existe no registo:

```sh
$ jk board show rpi3b-plus
```

```
rpi3b-plus 1.0.0 · Raspberry Pi 3 Model B+ · Raspberry Pi
  arch aarch64 · 4 CPUs · 1 GiB · 1.4 GHz · nível 2 (sem CI com hardware)
  kernel suportado   preempt_rt   (boot_cmdline: isolcpus=3 nohz_full=3 rcu_nocbs=3)
  recommended        rt [3] · io [2] · bg [0, 1]
  watchdog           /dev/watchdog0 · 15 s
  gpio               pinctrl-bcm2835
  IO fixo            board.act_led    sysfs led ACT, saída    fallback: —
                     board.cpu_temp   sysfs thermal_zone0
  i2c                i2c1 → /dev/i2c-1
  spi                spi0 → /dev/spidev0.0
  serial             uart0 → /dev/serial0
  net                eth → eth0 · wifi → wlan0
  fonte              oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0
```

O que isto te diz, e porque importa:

- **`recommended`** é a divisão das CPUs que a placa sugere: o núcleo 3 para o ciclo, o 2 para os drivers, 0 e 1 para o resto. É a omissão de `[resources]` (ref/01 §5.4).
- **`memory`** é a base do `J0380` (memória estimada acima de 70% da RAM).
- **`watchdog`** é a segunda camada do estado seguro (secção 12).
- **`IO fixo … fallback: —`**: o LED `ACT` **não tem fallback de hardware**. Um perfil de placa diz o estado que o hardware impõe quando o `jokerd` para (a chave `fallback` de `[io.*]`), e aqui não há nenhum. Guarda isto para a secção 12.

## 7. `joker.lab.toml`: o Pi 3 real

```toml
# joker.lab.toml — nó lab (Raspberry Pi 3 B+)
extends = ["joker.toml"]

[target]
board = "rpi3b-plus@1"

# A recomendação da placa, escrita por extenso.
[resources]
rt = { cpus = [3] }
io = { cpus = [2] }
bg = { cpus = [0, 1] }

# O LED ACT da placa, pelo driver sysfs.
[devices.pi]
driver = "sysfs"

[devices.pi.signals]
act_led = { led = "ACT", direction = "output" }

# A API também em TCP, sempre com mTLS, para a jk chegar do portátil sem SSH.
[security.api]
listen = ["tcp:[::]:7443"]
```

- **`[target] board`** com a major (`@1`): o lock vai fixar a versão exata e o digest (ref/02 §2.1.4). Uma placa que não existe é `J0172`.
- **`[resources]`** repete o `recommended`. Podias omiti-lo e o efeito era o mesmo (o `jk diff` mostrá-lo-ia como `# omissão`); escrevê-lo torna a decisão visível. O grupo `main` vai para a classe `rt` sozinho, porque `[resources.rt]` existe, e o `jk check` põe-no na CPU 3. Sobrepor `rt` a outra classe é `J0360`.
- **`[devices.pi]`** tem o mesmo nome do dispositivo simulado, e o sinal `act_led` o mesmo nome do sinal do modelo. Por isso o caminho `pi.act_led` existe nos dois mundos, e **nem a app nem o `joker.toml` mudam**. A placa oferece o mesmo LED como `board.act_led`, mas o prefixo `board.` é reservado ao IO fixo e não existe no simulador; um dispositivo com nome próprio mantém o caminho igual em todo o lado. O `board.act_led` só existe se o manifesto o referir, e referi-lo **e** ter um `[devices.pi]` com `led = "ACT"` seria o mesmo recurso do kernel duas vezes (`J0178`, ref/03 §2.3.2). Consequência: o `fallback` de um `[io]` da placa só conta pelo caminho `board.<nome>`.
- **`[security.api] listen`** acrescenta uma escuta às do `node.toml`, e nunca as retira (ref/02 §3.2.2). TCP é sempre TLS 1.3 com mTLS: não há chave para o desligar (ref/09 §2.1).

**Porque `sysfs`, e não `gpio`?** O LED `ACT` está ligado a uma linha GPIO do SoC, mas essa linha **já tem dono**: o driver `leds-gpio` do kernel, que a expõe como `/sys/class/leds/ACT` e lhe liga um *trigger* (por omissão, a atividade do cartão SD). O driver `gpio` do Joker pede as linhas ao kernel pelo chardev, e o kernel recusa uma linha que outro driver já pediu. O driver `sysfs` fala com o LED pela interface que o kernel oferece para isso: escreve `max_brightness` ou `0` em `brightness`. Ao abrir o LED, guarda o *trigger* atual e põe-no a `none`, para a atividade do SD não piscar por cima da app; ao fechar (paragem ordenada, `jk device stop`, uma geração que retira o sinal) repõe-no (ref/01, anexo A; documento 05, §10).

## 8. Verificar

```sh
$ jk check --env lab
```

```
✓ manifesto          joker.lab.toml ← joker.toml · placa rpi3b-plus@1 · 1 dispositivo (sysfs), 1 saída, 1 app, 1 grupo
✓ ligações           luz.led → pi.act_led (bool)
✓ recursos           rt [3] · io [2] · bg [0, 1]
✓ admissão RT        main 20ms → cpu 3: 5 µs declarados (0,03%)
✓ memória            estimada 38 MiB de 1 GiB
aviso[J0352]: o grupo `main` (20 ms) não tem kernel PREEMPT_RT neste alvo; jitter não garantido
aviso[W0205]: `pi.act_led` não tem fallback do dispositivo
   = nota: um LED em sysfs não tem watchdog próprio; se o jokerd morrer, fica no último valor
   = ajuda: em `mode = "prod"` isto é um erro; aceita-o com `accept_missing_fallback = "<justificação>"`,
            ou usa hardware com fallback
nota: joker.lab.lock atualizado (boards, apps.luz)
```

Guarda o `W0205`: vais vê-lo acontecer na secção 12.

### O que o `jk check` diz em produção

Muda `mode = "prod"` no `joker.toml` e volta a verificar:

```
erro[J0205]: `pi.act_led` não tem fallback do dispositivo
 --> joker.lab.toml:14:1
   |
14 | [devices.pi]
   | ^^^^^^^^^^^^ driver sysfs: sem fallback; a placa rpi3b-plus não declara fallback para o LED ACT
   = ajuda: `accept_missing_fallback = "<justificação ≥ 10 caracteres>"` em [outputs."pi.act_led"];
            o `jk lock` copia-a para [accepted]
```

O mesmo aviso passou a erro, com o mesmo número. É a regra de `dev` e `prod` (documento 03, §15). Repõe `mode = "dev"`.

## 9. O lock

```sh
$ jk build --env lab                    # compila a app e pré-compila o AOT para aarch64
$ jk lock --env lab --node lab
```

```
joker.lab.lock
  = boards."rpi3b-plus@1"   1.0.0   (oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0)
  = apps.luz                0.1.0   (path:apps/luz)
  + runtime 1.0.0 · node lab
```

```toml
# This file is generated by jk. Do not edit by hand.
version = 1
runtime = "1.0.0"
node = "lab"

[boards."rpi3b-plus@1"]
resolved = "1.0.0"
digest = "sha256:…"
source = "oci://hub.example.com/joker/boards/rpi3b-plus:1.0.0"
oci = "sha256:…"

[apps.luz]
version = "0.1.0"
digest = "sha256:…"
wit = "joker:runtime@1.0.0"
source = "path:apps/luz"
```

Um lock por manifesto de entrada (`joker.lab.toml` → `joker.lab.lock`, ao lado do `joker.sim.lock` da parte 1), porque o simulador e a placa resolvem coisas diferentes (ref/02 §2.1.1). Com `--node`, a `jk` pergunta ao nó, só com leituras, os factos que dependem dele e escreve `runtime` e `node`; aplicar este lock noutro nó dá `J0480`. Aqui não há descoberta nem fallback a capturar: o `sysfs` não tem nenhum. Versiona o `joker.lab.lock`.

## 10. Aplicar, confirmar, gerações

```sh
$ jk apply --env lab --node lab --confirm 5m
```

```
→ lab (dev) · de --node · ssh://pi@lab.local
plano da geração 1
  + placa rpi3b-plus@1 · recursos rt [3] io [2] bg [0, 1]
  + dispositivo pi (sysfs, 1 sinal)
  + app luz 0.1.0 (aarch64 AOT, 208 KiB)
  + grupo main 20ms (rt, cpu 3)
  + api tcp:[::]:7443 (mTLS)                       ← mexe na API: pede confirmação
aplicado. confirmação pendente: `jk confirm` em 5 min, ou rollback automático
```

O LED `ACT` do Pi pisca a 1 Hz. **A app é a mesma da parte 1, com o mesmo digest.**

O `--confirm` parece exagerado para um LED, mas é o hábito certo, e aqui tem uma razão concreta: esta geração mexe na API. Numa instalação remota, uma geração que corta o acesso desfaz-se sozinha se ninguém a confirmar (ADR 0015). Por isso, **confirma pelo caminho novo**: muda o contexto `lab` para TCP e confirma por lá. Se a escuta TCP não funcionasse, não conseguias confirmar, e a geração voltava atrás sozinha.

```sh
$ jk config set nodes.lab.address lab.local:7443      # ou à mão, em [nodes.lab] do config.toml
$ jk confirm --node lab
```

```
→ lab (dev) · de --node · lab.local:7443 (mTLS, cert:admin-dl)
geração 1 confirmada
```

O certificado do nó verificou pela tua CA e tem o nome `lab` no SAN; o teu certificado verificou pela CA de clientes do nó. Em `dev`, sem `[security.roles]`, qualquer principal autenticado é `admin` (ref/01 §5.20).

Agora uma segunda geração. Muda `half_period` para `"250 ms"` no `joker.toml` e aplica:

```sh
$ jk apply --env lab --node lab --confirm 5m && jk confirm --node lab
$ jk generations --node lab
```

```
gen  estado       origem   quem            quando                hash
  2  confirmed    local    cert:admin-dl   2026-09-25 10:31:07   sha256:9e41…   ~ apps.luz.params.half_period 500 ms → 250 ms
  1  confirmed    local    cert:admin-dl   2026-09-25 10:24:52   sha256:3b0c…
```

A geração 2 só recarregou um parâmetro: a app não reiniciou. Volta atrás:

```sh
$ jk rollback 1 --node lab
```

```
→ lab (dev) · de --node · lab.local:7443
geração 1 reativada (sha256:3b0c…) · origem rollback
```

Um rollback **reativa** a geração anterior, com o mesmo número e o mesmo hash, e o histórico regista a ativação com a origem `rollback` (ref/00 §5). Mas o teu `joker.toml` ainda diz 250 ms: o nó e o repositório divergem. O rollback é para emergências; a verdade está no Git. Repõe o ficheiro (`git checkout joker.toml`).

## 11. Escrita manual e árbitro

Com a app a piscar, força o LED aceso durante 30 s:

```sh
$ jk write pi.act_led true --ttl 30s --reason "teste do árbitro" --node lab
```

```
→ lab (dev) · de --node · lab.local:7443
pi.act_led: escrita manual (manual:4) durante 30 s
  envelope: safe = false · sem gama, taxa nem interlock
  prevalece sobre: control:12 (app:luz)
confirmar? [s/N] s
aceite · manual:4 · até 10:42:37
```

O LED fica aceso. A app continua a propor o pisca, mas o nível `manual:4` tem prioridade maior (1 é a mais alta). Passados 30 s, o nível manual expira sozinho e o pisca volta (documento 16, A5 e A10). O `--ttl` e o `--reason` são obrigatórios e não têm variável de ambiente, de propósito (documento 07, §1).

Enquanto dura, pergunta ao Joker quem manda nesta saída:

```sh
$ jk explain pi.act_led --node lab
```

```
pi.act_led   bool · saída · dispositivo pi (sysfs, led ACT)             joker.lab.toml:18
  envelope   safe = false                                                joker.toml:9
  grupo dono main 20ms (a app de maior prioridade que a escreve em ciclo, A11)
  níveis     manual:4     true    user:cert:admin-dl · "teste do árbitro" · até 10:42:37   ← ativo
             control:12   false   app:luz · lease 60 ms (omissão, 3 × 20 ms)
  aplicado   true   (overridden: control:12 perde para manual:4)
  último     control:12 → overridden(manual:4) · manual:4 → accepted
```

E o registo de decisões, que guarda cada escrita com o resultado e o motivo:

```sh
$ jk decisions --output pi.act_led --since 1m --node lab
```

```
10:42:07.300  manual:4     true   accepted              (user:cert:admin-dl, "teste do árbitro")
10:42:07.320  control:12   false  overridden(manual:4)
…
10:42:37.300  manual:4     —      expirado (ttl 30 s)
10:42:37.320  control:12   true   accepted
```

## 12. Quando as coisas correm mal: o estado seguro em três camadas

Três experiências, pela ordem certa (documento 01, §8).

**(a) A app para.**

```sh
$ jk app stop luz --node lab
```

O LED apaga-se em ≤ 60 ms. Ninguém escreveu `false`: o lease da app expirou, não ficou nenhum nível ativo, e o árbitro aplicou o `safe` (documento 16, A6). **Primeira camada: o lease.** Cobre a app morta, encravada ou num *crashloop*. Volta a ligá-la com `jk app start luz --node lab`.

**(b) O `jokerd` encrava.** Não o vais provocar, mas está lá: a thread supervisora do `jokerd` arma o watchdog de hardware da placa (`/dev/watchdog0`, 15 s, o que o `jk board show` mostrou) e o `WatchdogSec` do systemd. Se um grupo encravar, o supervisor reinicia a runtime; se o processo inteiro deixar de responder, o watchdog reinicia a placa. **Segunda camada: o supervisor.** Cobre o que a primeira não cobre, porque um lease só expira se houver quem o conte.

**(c) O `jokerd` morre.** Espera que o LED esteja aceso e mata o processo:

```sh
lab$ sudo systemctl kill -s KILL jokerd
```

**O LED fica aceso.** Morto o processo, ninguém escreve no `brightness` nem repõe o *trigger*, e o kernel mantém o último valor. O systemd reinicia o `jokerd` em 1–2 s; o `jokerd` arranca com todas as saídas em `safe` (documento 16, A8), o LED apaga-se, e a `luz` volta a piscar. Com o LED apagado no instante do `kill`, não verias diferença nenhuma: faz as duas vezes.

Esta é a **lição honesta da parte 2: o LED `ACT` não tem fallback de hardware.** A terceira camada do estado seguro é o **fallback do dispositivo** (M1): o próprio equipamento vai para um valor seguro quando deixa de ouvir o Joker, e é a única camada que cobre o `jokerd` morto, a placa sem energia e o cabo cortado. Um LED em `sysfs` não tem nada disso, e a placa não declara nenhum. É exatamente o `W0205` da secção 8, e em `prod` seria o erro `J0205`.

Num LED não faz mal. Num aquecedor ou num motor, "fica como estava" pode ser "fica ligado". É por isto que o Joker, em produção, exige equipamentos com watchdog próprio, ou uma justificação escrita (`accept_missing_fallback`) que fica no lock com quem a deu e quando. Na parte 4 vais ter um equipamento que se protege sozinho: o firmware de bancada do ESP32 apaga o LED dele se não ouvir nada durante 2 s.

## 13. O que aconteceu por baixo

- **Identidade sem portas abertas.** O nó sem identidade só escuta o socket Unix; o enrolamento corre por esse socket (por SSH); a chave nasce e fica no nó; a escuta TCP só existe depois, com mTLS obrigatório, e porque uma geração a pediu (ref/02 §3.2; ref/09 §2.1; ADR 0025).
- **A geração** é o manifesto efetivo, o lock e os artefactos, com um hash. O `jk apply` envia-a, o nó verifica cada artefacto pelo digest do lock, ativa-a, e espera pela confirmação dentro da janela; sem ela, volta à última confirmada (documento 01, §15; ADR 0015).
- **A placa** entrou no lock pelo digest dos bytes do ficheiro. Dela vieram a omissão de `[resources]`, a memória do `J0380` e o watchdog do supervisor. O `jokerd` pôs o grupo `main` num cpuset só com a CPU 3, que o `isolcpus` tirou ao escalonador geral (documento 01, §2).
- **O mesmo núcleo, outro driver.** O tick é o da parte 1; a diferença é que o commit vai para o driver `sysfs` em vez do `sim`. O árbitro e o envelope não sabem, nem precisam de saber, de onde vem o LED.
- **O árbitro** decide por nível e por lease, e nunca por quem chegou primeiro: um humano em `manual:4` ganha a uma app em `control:12` até ao fim do TTL, e tudo fica no registo de decisões (documento 16, §1; ADR 0011).

## 14. Exercícios

1. Com `jk dev --env lab --node lab`, muda o `half_period` e vê o LED real mudar sem `jk apply`. O que aparece no `jk generations` depois?
2. Faz `rt = { cpus = [2] }` no `joker.lab.toml`, sem mexer no `io`. Lê o que o `jk check` diz e explica porquê.
3. Escreve `jk write pi.act_led false --ttl 10m --reason "…"` e, noutro terminal, uma segunda escrita com o mesmo nível a partir de outro utilizador (`ssh pi@lab.local jk write … --node local`). O que responde a segunda? E com `--takeover`?
4. Faz a experiência 12(c) com o LED apagado e depois aceso. Explica a diferença a alguém, usando as palavras "lease", "supervisor" e "fallback do dispositivo".

```sh
$ git add -A && git commit -qm "trilha(02): o Pi 3 como nó lab; LED ACT real com a luz 0.1"
```

**Estado no fim desta parte:** app `luz` **0.1, sem mudanças**: só os manifestos mudaram. Ficheiros novos: `joker.lab.toml`, `joker.lab.lock`. Fora do projeto: `~/org-ca/`, o contexto `lab` em `~/.config/joker/config.toml`, e o nó `lab` enrolado, na geração 1 confirmada.

**Próximo:** [Parte 3 — Um sensor real: a temperatura do CPU, alarmes, historian e a API](03-sensor-temperatura.md).
