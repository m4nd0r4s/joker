# Parte 11 — Produção e frota: assinaturas, segredos, o Deck e dois nós

**Objetivo:** pôr o projeto em `mode = "prod"`, com assinaturas e um segredo, e passar a atualizar os dois Raspberry Pi a partir do Git, pelo Deck, com um canário que trava uma versão má antes de ela chegar ao outro nó.

**Vais aprender:** o que o `prod` muda · chaves por papel e o bundle de confiança · `accept_missing_fallback` e `max_manual` · segredos cifrados por destinatário · o Deck MVP · enrolamento com token · estado desejado, ondas e confirmação · `halt_if` e rollback · um nó offline · o que o Deck não pode fazer.

**Antes de começar:** partes 1–10. O `lab` (Pi 3) corre `joker.lab-link.toml` (geração 15), com o ESP32 como Joker Micro por Wi-Fi (partes 9–10) e a `luz` 0.6; a CA `~/org-ca` é a da parte 2 (`--org bancada`). O `zero` está parado desde a parte 7, na geração 2 do `joker.zero.toml` (a `luz` 0.5): liga-o (`zero$ sudo systemctl start jokerd`). **O `esp` do `zero` fica `bad(comm_failure)`**, e o `esp_offline` ativo: desde a parte 9, o firmware do ESP32 é o Joker Micro, que já não serve Modbus TCP. É o comportamento tolerante da 0.4 (parte 5): o `zero` continua a piscar pelo seu LED `ACT`. O portátil e os dois Pi estão na mesma rede.

---

## 1. Passar a `mode = "prod"`

Em `dev`, o Joker deixa passar o que ainda não está pronto e avisa. Em `prod`, os mesmos códigos passam a erros e as assinaturas passam a ser obrigatórias (ref/01 §5.2). Muda uma linha do `joker.toml`:

```toml
[project]
name  = "bancada"
joker = "1.0"
mode  = "prod"
```

### 1.1 O que o `jk check` diz

```sh
$ jk check --env lab-link
```

```
error[J0205]: fallback do dispositivo em falta ou incoerente com `safe`
  --> joker.toml:6:1
   |
 6 | [outputs."pi.act_led"]
   | ^^^^^^^^^^^^^^^^^^^^^^ o driver `sysfs` não tem fallback: se o jokerd parar, o LED ACT fica como estava
   = ajuda: numa saída sem consequências físicas, justifica-o com accept_missing_fallback = "…" (ref/01 §5.10)

error[J0794]: nó em `prod` com um MCU cuja geração é `dev`
  --> micro/esp/joker.toml:5:9
   |
 5 | mode  = "dev"
   |         ^^^^^ o dispositivo esp (joker-link, joker.lab-link.toml) corre este manifesto
   = nota: um nó em `prod` só envia a um MCU blobs de geração `prod` (bit DEV a 0, ref/08 §2.16)
   = ajuda: passa o manifesto do MCU a mode = "prod"
2 erros · em `dev` o primeiro era o aviso W0205, e o segundo não existia
```

São dois problemas, um de cada lado do cabo. O primeiro é o aviso que viste na parte 2, agora a sério. Um LED indicador não tem consequências físicas, e o Joker aceita isso se o disseres por escrito. Aproveita para limitar as escritas manuais nesta saída a 30 minutos:

```toml
[outputs."pi.act_led"]
safe                    = false
max_manual              = "30m"
accept_missing_fallback = "LED ACT da placa: indicador, sem consequências físicas"
```

**O MCU também passa a `prod`.** O `[project] mode` do `micro/esp/joker.toml` é o modo do blob que o `lab` leva ao ESP32, e ficou em `dev` desde a parte 9. Muda-o (`mode = "prod"`) e volta a verificar:

```sh
$ jk check --env lab-link
```

```
error[J0786]: placa sem saídas seguras no reset (`reset_safe = false`); aceitável com `accept_unsafe_reset`
  --> micro/esp/joker.toml:8:9
   |
 8 | board = "esp32-devkit@1"
   |         ^^^^^^^^^^^^^^^^ até ao passo 6 do arranque, os pinos ficam no estado de reset do hardware
   = nota: em `dev` era o aviso W0786 da parte 9
   = ajuda: se o hardware do site o tornar aceitável, justifica-o com accept_unsafe_reset = "…" no [target] (ref/01 §5.22)
1 erro
```

Em `prod`, as verificações do MCU são erros como as do nó. A placa `esp32-devkit` declara `reset_safe = false`: entre o reset e o momento em que o firmware configura os pinos, o Joker não garante nada sobre as saídas. Na bancada, sabes o que o hardware faz: o GPIO2 é um pino de arranque do ESP32, puxado a baixo, e o LED fica apagado no reset. Escreve-o no `[target]` do manifesto do MCU:

```toml
# micro/esp/joker.toml (o [project] e o [target] mudam)
[project]
name  = "bancada-esp"
joker = "1.0"
mode  = "prod"

[target]
board               = "esp32-devkit@1"
accept_unsafe_reset = "bancada: o LED do GPIO2 está num pino de arranque puxado a baixo, por isso fica apagado no reset"
```

O `J0786` passa a aviso. A aceitação não muda o firmware (os pinos continuam no estado de reset até ao passo 6, ref/08 §2.16): fica só escrito quem a aceitou, porquê e para que placa. A chave no lock é a placa resolvida, `esp32-devkit@1`: noutra placa, a aceitação não vale (`J0440`).

As duas justificações vão para os locks, com quem e quando. Há um lock por manifesto de entrada, e o MCU tem o seu (`micro/esp/joker.lock`, parte 10); por isso fazem-se os três:

```sh
$ jk lock --env lab-link && jk lock --env zero && jk lock --manifest micro/esp/joker.toml
```

```toml
# joker.lab-link.lock (excerto)
[accepted]
accept_missing_fallback."pi.act_led" = { reason = "LED ACT da placa: indicador, sem consequências físicas", by = "user:dl", at = "2026-09-25T09:12:40Z" }
```

```toml
# micro/esp/joker.lock (excerto)
[accepted]
accept_unsafe_reset."esp32-devkit@1" = { reason = "bancada: o LED do GPIO2 está num pino de arranque puxado a baixo, por isso fica apagado no reset", by = "user:dl", at = "2026-09-25T09:13:05Z" }
```

O blob do MCU passa a ser, também ele, um artefacto de `prod`: sem o bit `DEV`, e assinado. Quem o assina é o `jk publish`, com a chave do papel `config`, como as gerações dos nós Linux (secção 3; ref/02 §2.5.3). É o fim do `W0779` da parte 9: em `prod`, o firmware recusa um blob sem assinatura.

Quando esta geração chegar ao nó (secção 3), uma escrita manual acima do limite é recusada, e não encurtada em silêncio:

```sh
$ jk write pi.act_led true --ttl 2h --reason "teste" --node lab
→ lab (prod) · de --node
error[J-E1102]: TTL acima de `max_manual`: pi.act_led aceita no máximo 30m
```

O `zero` é de nível `dev` (documento 23, §7), e isso não muda com o `prod`: `mode = "prod"` quer dizer "com as garantias de produção", e não "hardware de produção".

### 1.2 Chaves, uma por papel

Em `prod`, cada artefacto tem de vir assinado por uma chave com o papel certo (ref/02 §2.5.3). Uma chave por papel é o que impede que a chave de todos os dias (a da configuração) seja a chave de tudo:

```sh
$ jk key gen config --kind signing
$ jk key gen apps --kind signing
$ jk key gen segredos --kind signing
$ jk key gen deck --kind signing
$ jk key gen dl --kind secrets
```

```
ed25519:2b7f41c09e8d3a5f6c1e2d4b7a9f0c3e  ~/.local/share/joker/keys/config.key · keys/config.pub
ed25519:91c4e0a7b3d2f6158e4c7a0b9d3f2e61  ~/.local/share/joker/keys/apps.key · keys/apps.pub
ed25519:4d0a6e2c8b1f7394a5e0c6d2b8f41a73  ~/.local/share/joker/keys/segredos.key · keys/segredos.pub
ed25519:e6f39b0c4a7d2e81f5c0a3b6d9e24c17  ~/.local/share/joker/keys/deck.key · keys/deck.pub
x25519:7a1c5e9f3b0d4c28e6a1f7b3c5d09e42   ~/.local/share/joker/keys/dl.x25519.key · keys/dl.x25519.pub
```

As privadas ficam fora do projeto (`J0913` se estiverem dentro); a pasta `keys/` só tem `.pub`. As quatro primeiras são de assinatura (Ed25519): `config` assina gerações, `apps` as apps, `segredos` as entradas de segredos, `deck` os documentos do Deck. A última é a tua chave de *escrow* (X25519), a que te deixa ler e voltar a cifrar segredos; diz à `jk` onde ela está, com `secrets_key = "~/.local/share/joker/keys/dl.x25519.key"` em `[user]` do `~/.config/joker/config.toml` (ref/02 §2.3.2).

### 1.3 O bundle de confiança

Os nomes dos ficheiros não dão papéis a ninguém. Quem diz "esta chave assina apps" é o **bundle de confiança**, assinado pela raiz da organização, que está na CA da parte 2 (ref/10 §2.4.3):

```sh
$ jk ca bundle ~/org-ca --key config=keys/config.pub --key apps=keys/apps.pub \
    --key secrets=keys/segredos.pub --deck bancada-deck=keys/deck.pub \
    --deck-url https://portatil.local:8443 --min-mode prod --out ~/org-ca/trust.cose
```

```
bundle de confiança · org bancada · versão 2 · assinado por ed25519:5e0c9a17… (raiz)
  config   ed25519:2b7f41c0…   secrets  ed25519:4d0a6e2c…
  apps     ed25519:91c4e0a7…   deck     ed25519:e6f39b0c… (bancada-deck)
  authority bancada-deck · deck_urls https://portatil.local:8443 · min_mode prod
```

A raiz só serve para isto (e para trocar âncoras). Tira-a do portátil: `mv ~/org-ca/root.key` para uma pen USB, e guarda a pen. O Deck nunca a lê (documento 20, §4.4.2).

### 1.4 O manifesto só restringe

```toml
[security]
trust = { config = ["keys/config.pub"], apps = ["keys/apps.pub"], secrets = ["keys/segredos.pub"] }

[security.roles]
operator = ["cert:CN=admin-*"]
admin    = ["cert:CN=admin-*"]
```

`[security.trust]` diz "neste projeto, só estas", e nunca pode dar a uma chave um papel que o bundle não lhe deu (`J0292`; vês o erro na secção 3). `[security.roles]` é preciso porque em `prod` as omissões são as restritivas: sem ele, o teu certificado de utilizador da parte 2 só lia (ref/01 §5.20).

## 2. Segredos: a telemetria para o `mosquitto`

A versão 0.7 do projeto publica a temperatura e os alarmes por MQTT para um `mosquitto` no Pi 3. Quem publica é o **nó**, por um uplink do manifesto, e não a app: uma app Wasm não abre sockets. No `joker.toml` (ref/01 §5.17.3):

```toml
[telemetry.uplink.bancada]
kind     = "mqtt"
broker   = "mqtt://lab.local:1883"
classes  = { alarms = "alarm.**", high = ["pi.cpu_temp", "calc.temp_media"] }
live     = { every = "5s" }
username = "bancada"
password = "secret:mqtt_bancada"
```

```
error[J0296]: ligação sem TLS ou sem segurança (`mqtt://`, `http://`, OPC UA `None`)
  --> joker.toml:61:12
   |
61 | broker   = "mqtt://lab.local:1883"
   |            ^^^^^^^^^^^^^^^^^^^^^^^ em `prod`, a palavra-passe iria em claro pela rede
   = ajuda: usa mqtts:// (porta 8883)
```

Passa a `broker = "mqtts://lab.local:8883"`. O `mosquitto` não é do Joker; o certificado dele faz-se com `openssl`, pela emissora da tua CA, que os nós já conhecem:

```sh
$ openssl ecparam -name prime256v1 -genkey -noout -out mosquitto.key
$ openssl req -new -key mosquitto.key -subj "/CN=lab.local" -addext "subjectAltName=DNS:lab.local" -out mosquitto.csr
$ openssl x509 -req -in mosquitto.csr -CA ~/org-ca/ca.crt -CAkey ~/org-ca/ca.key -CAcreateserial -days 365 -copy_extensions copy -out mosquitto.crt
$ scp mosquitto.key mosquitto.crt ~/org-ca/ca.crt pi@lab.local:/tmp/ && rm mosquitto.key mosquitto.csr
```

```sh
lab$ sudo apt install -y mosquitto && sudo install -m 0640 -g mosquitto /tmp/mosquitto.* /tmp/ca.crt /etc/mosquitto/certs/
lab$ printf 'listener 8883\ncafile /etc/mosquitto/certs/ca.crt\ncertfile /etc/mosquitto/certs/mosquitto.crt\nkeyfile /etc/mosquitto/certs/mosquitto.key\npassword_file /etc/mosquitto/passwd\nallow_anonymous false\n' | sudo tee /etc/mosquitto/conf.d/bancada.conf
lab$ sudo mosquitto_passwd -c /etc/mosquitto/passwd bancada && sudo systemctl restart mosquitto
```

A palavra-passe que escolheste no `mosquitto_passwd` é o segredo. Entra no projeto sem nunca ficar em claro (ref/02 §2.4):

```sh
$ jk secret recipient add --user dl --key keys/dl.x25519.pub
$ jk secret set mqtt_bancada --description "mosquitto do lab" --sign-key ~/.local/share/joker/keys/segredos.key
valor: ••••••••••••  (outra vez) ••••••••••••
$ jk secret recipient add --node lab
$ jk secret recipient add --node zero
$ jk secret list
```

```
mqtt_bancada  rev 1  string  nós: todos  signer ed25519:4d0a6e2c…  destinatários: node:lab, node:zero, user:dl
```

O valor nunca é argumento da linha de comandos (ficaria no histórico da shell). O `recipient add --node` obtém a chave KEM do nó **do certificado dele**, verificado pela tua CA (`J0929` se não estiver atestada): ninguém consegue fazer-te cifrar para uma chave que não é do nó. O `secrets.toml.enc` vai para o Git (ADR 0024): tem os nomes e os destinatários em claro, e os valores cifrados para três chaves que o Deck não tem.

```sh
$ git add -A && git commit -m "bancada: prod, chaves, telemetria MQTT com segredo"
```

## 3. O Deck no portátil

O Deck é outro binário, com o seu `deck.toml`. O exemplo do documento 20, §4.4.2, é exatamente o desta bancada: copia-o para `~/deck/deck.toml`, fora do projeto. Tem o `id` `bancada-deck` e o `url` que puseste no bundle, o repositório do projeto (`[git] repo = "~/joker-bancada"`, ramo `main`, lido a cada 30 s), a CA da parte 2 (`[ca] dir = "~/org-ca"`), SQLite e o armazém embutido (em produção, PostgreSQL e um registo OCI), e em `[signing]` a chave `deck`. É a única chave privada que o Deck tem: numa instalação a sério, o Deck corre noutra máquina, e as chaves `config`, `apps` e `segredos` nunca lá chegam.

No repositório do projeto, o `fleet.toml` diz que nó corre que manifesto e em que coorte. O `zero` é o **canário**: o nó mais fraco é o que recebe primeiro.

```toml
# fleet.toml
version = 1

[nodes.zero]
manifest = "joker.zero.toml"
cohort   = "canario"

[nodes.lab]
manifest = "joker.lab-link.toml"
cohort   = "estavel"

[rollout]
wait    = "10m"
confirm = { mode = "auto", window = "15m", stable_for = "5m" }
halt_if = ["overruns_rate > 0.001", "trips_rate > 0", "rollbacks > 0"]

[[rollout.wave]]
cohorts = ["canario"]

[[rollout.wave]]
cohorts = ["estavel"]
```

```sh
$ git add fleet.toml && git commit -m "bancada: fleet.toml"
$ joker-deck serve --config ~/deck/deck.toml
```

```
bancada-deck · https://portatil.local:8443 · sqlite:deck.db · armazém embutido · chave deck ed25519:e6f39b0c…
git    main 3c8e1f2 "bancada: fleet.toml" · 2 nós, 2 coortes, 2 ondas
lab    geração sha256:61f0… · error[J0915]: artefacto ou geração sem assinatura em `prod` · nada emitido
zero   geração sha256:a3d7… · error[J0915]: artefacto ou geração sem assinatura em `prod` · nada emitido
```

É o momento "o que o Deck diz". O Deck validou as duas gerações como o `jk check`, e parou onde tinha de parar: não tem a chave `config`, e por isso não consegue assinar o que leu do Git. Quem assina és tu, com as tuas chaves, e envias para o armazém do Deck:

```sh
$ jk publish --env lab-link --env zero --registry oci://portatil.local:8443/bancada \
    --sign-key config=~/.local/share/joker/keys/config.key --sign-key apps=~/.local/share/joker/keys/apps.key
```

```
joker.lab-link.toml  geração sha256:61f0…  config ✓ · luz 0.6.0 apps ✓ · status_light_ctl 1.0.0 apps ✓ · blob do MCU esp (prod) config ✓ · 7 artefactos enviados
joker.zero.toml      geração sha256:a3d7…  config ✓ · 2 artefactos novos (os outros já lá estavam, por digest)
```

O blob do MCU (`micro/esp/joker.toml`, agora em `prod`) é assinado aqui, com a mesma chave `config` da geração do `lab`, que o leva ao ESP32 pelo link; o ESP32 verifica-o contra o papel `config` do seu bundle antes de o abrir. O hash da geração é o mesmo na `jk` e no Deck, porque é o mesmo cálculo com os mesmos crates (ref/10 §2.6.2).

### 3.1 Enrolar os nós com um token

```sh
$ joker-deck token create --node lab --ttl 1d
jkt1_bancada_5e0c9a17d2b84f6e91a3c07d58e2f4b1_K7Q2M9XW4TPA6RZC3HNV8JDF5LBY1SEG  (lab · expira 2026-09-26T10:05Z · uso único)
$ jk node enroll ssh://pi@lab.local --deck https://portatil.local:8443 --token jkt1_bancada_5e0c9a17… --force
```

```
→ ssh://pi@lab.local · já enrolado com ~/org-ca: --force gera uma identidade nova
warning[J0933]: chave de identidade em ficheiro, sem TPM nem elemento seguro
certificado CN=lab, O=bancada · até 2027-09-25 · emitido por bancada-deck
âncoras: ed25519:5e0c9a17… (confere com o token) · bundle versão 2 · authority bancada-deck
policy.allow_dev = false
```

O token leva o `kid` da raiz: é assim que o nó verifica o bundle que recebe sem confiar na rede (ref/10 §2.5.1). É o nó que liga ao Deck, e a chave nunca sai dele. Faz o mesmo para o `zero`, com outro token. O Pi 3 não tem TPM, e o `J0933` fica visível em cada nó com a chave em ficheiro (documento 23, §5.4).

```sh
$ jk agent status --node lab
autoridade   bancada-deck · https://portatil.local:8443 · https, long-poll · ligado há 40 s · desvio 9 ms
desejado     nenhum aceite ainda (seq 0) · fila 0 · pausa: não · alterações locais: hold
```

E o log do Deck:

```
lab    enrolado · hello agent/1.0 · relatório completo
lab    error[J0922]: segredo não cifrado para este nó: mqtt_bancada foi cifrado para outro certificado · em espera
```

O `--force` deu ao `lab` uma identidade nova, e com ela uma chave KEM nova (ref/02 §3.2.3). O segredo estava cifrado para a antiga. O Deck não o consegue ler, mas vê que o `wrap` é para um certificado que já não é o do nó, e não manda uma geração que ia falhar no nó. A correção é a de sempre:

```sh
$ jk secret recipient add --node lab && jk secret recipient add --node zero
$ git commit -am "bancada: segredos para as identidades novas" && jk check --env zero --node zero
```

O `jk check --node` já conhece o bundle do nó, e é aqui que o `J0292` apareceria se o `[security.trust]` pusesse, por exemplo, `keys/deck.pub` na lista `apps`: "tem no bundle só o papel `deck`". O manifesto não alarga a confiança.

O Deck vê o commit e faz o primeiro rollout: a geração de hoje, agora em `prod`, primeiro no `zero` e depois no `lab`, pelas ondas (secção 4). A partir daqui os dois nós recusam gerações `dev` (o bundle diz `min_mode = prod`): o `jk dev --node lab` responde `J0906`. O ciclo rápido de desenvolvimento passa a viver no simulador, e o hardware da frota só muda por commits.

## 4. Um rollout bom: a 0.7

A 0.7 acrescenta um modo: com o alarme `cpu_quente` ativo, a luz passa a `alert`.

```rust
// apps/luz/src/lib.rs — o que muda da 0.6 para a 0.7
#[joker::app(name = "luz", version = "0.7.0")]
pub struct Luz {
    // … as portas da 0.6, iguais …
    #[input] quente: Input<bool>,        // alarm.cpu_quente.active
}

    // na tarefa `pisca`, logo a seguir ao `if !st.ligado { … }`:
    if io.quente.get_or(false) {
        io.sinal.alert();
        return Ok(());
    }
```

No `joker.toml`, o `bind` da `luz` ganha `quente = "alarm.cpu_quente.active"` (cada alarme cria esse sinal, ref/01 §5.15). Depois, o caminho de uma versão nova em `prod`:

```sh
$ jk test                                                   # os cenários, incluindo o falha_esp.scenario.toml
$ jk bench luz --env lab-link --node lab          # a app mudou de digest: o WCET antigo é J0431
$ jk bench luz --env zero --node zero
$ jk check --env lab-link && jk check --env zero
$ jk publish --env lab-link --env zero --registry oci://portatil.local:8443/bancada \
    --sign-key config=~/.local/share/joker/keys/config.key --sign-key apps=~/.local/share/joker/keys/apps.key
$ git commit -am "luz 0.7: modo alert com o cpu_quente"
```

No terminal do Deck:

```
10:02:11 git      main a41c0de "luz 0.7: modo alert com o cpu_quente"
10:02:12 rollout  01JAXQ7M… · lab sha256:7d2e… ✓ · zero sha256:c0a9… ✓ · 2 ondas
10:02:12 onda 1   canario: zero · seq 2
10:03:40 zero     applying → confirming · janela 15m · stable_for 5m
10:08:52 zero     confirmed · overruns_rate 0 · trips_rate 0 · rollbacks 0
10:18:52 onda 1   passou (10m sem halt_if) · onda 2 estavel: lab · seq 2
10:25:30 lab      confirmed · rollout concluído
```

No `zero`, enquanto confirma:

```sh
$ jk agent desired --node zero
seq 2 · rollout 01JAXQ7M… onda 1 (canario) · luz 0.7: modo alert com o cpu_quente
fase generation · confirming · 3m12s de 5m seguidos saudáveis · janela até 10:18:40
saúde  H1 grupos ok · H2 apps ok · H3 sem trips · H4 dispositivos como antes · H5 sem alarmes ≥ critical · H7 report-ack ✓
```

O `auto` confirma-se sozinho pela saúde **e** por uma troca autenticada com o Deck depois da troca (H7): uma geração que cortasse a rede ao nó nunca se confirmava a si própria (ADR 0015, ref/10 §3.5). O `esp` do `zero` já estava em baixo antes da troca, e o H4 só compara com o que estava `up`.

Agora vê a 0.7 a fazer o que diz, no `lab`:

```sh
lab$ stress-ng --cpu 4 --timeout 5m
```

A média de 10 s passa os 70 °C, o `cpu_quente` fica ativo 10 s depois, e o LED `ACT` passa ao padrão de alerta. No `mosquitto`, `mosquitto_sub -h lab.local -p 8883 --cafile ~/org-ca/ca.crt -u bancada -P … -t 'joker/#'` mostra a transição do alarme.

## 5. Um rollout mau: a 0.7.1

A 0.7.1 é uma "melhoria": em vez da média do `calc`, uma mediana dos últimos 2 048 ticks (41 s), calculada na tarefa de ciclo.

```rust
fn mediana(janela: &mut Vec<f64>, x: f64) -> f64 {
    if janela.len() == 2048 { janela.remove(0); }   // desloca 2 047 valores
    janela.push(x);
    let mut v = janela.clone();                      // 16 KiB copiados a cada tick
    v.sort_by(f64::total_cmp);                       // ~22 000 comparações, a cada 20 ms
    v[v.len() / 2]
}
```

Os testes passam (no portátil, o custo não se vê). O `jk bench` mede-o:

```
lab   luz.pisca  p99,9 = 0,41 ms   máx = 0,66 ms   rpi3b-plus
zero  luz.pisca  p99,9 = 12,6 ms   máx = 15,3 ms   rpi-zero-w
```

```sh
$ jk check --env zero
admissão RT (M23): cpu 0 · main 20 ms · 63% (limite 70%) · ok
warning[J0352]: grupo ≥ 10 ms sem PREEMPT_RT: jitter não garantido
```

No Zero, sem Cranelift, o Wasm é interpretado (documento 23, §5.2), e a tarefa passa de 0,4 para 12,6 ms. A admissão aceita-a: 63% cabem no teste de escalonabilidade. Mas o `bench` mede a tarefa sozinha, e no Zero há um núcleo para tudo: o historian, o uplink TLS, o Wi-Fi, as tentativas ao `esp` que não responde. `jk publish`, commit, e o Deck começa pelo canário:

```
11:40:05 onda 1   canario: zero · seq 3
11:40:31 zero     applying → confirming
11:41:02 zero     overruns_rate 0,061 (94 de 1 550 ticks) · halt_if "overruns_rate > 0.001"
11:41:02 rollout  01JAXW2P… halted · onda 2 não emitida · reject → zero (seq 3, generation)
11:41:04 zero     rolled_back · error[J0969]: geração rejeitada pelo Deck durante a confirmação: rollback
11:41:04 zero     seq 4: a geração anterior (sha256:c0a9…), já reativada · fase saltada
```

No `zero`, entre 11:40:31 e 11:41:04, o LED pisca aos solavancos (cada overrun salta um tick) e o `jk status --node zero` mostra `main 20 ms overrun · overruns 94` e o alarme `joker.group.main.overrun`. O H1 nunca ficou 5 minutos seguido: sem o Deck, a janela expirava às 11:55:31 e o nó voltava atrás na mesma, com `J0961`. O Deck só foi mais depressa. O rollback **reativa** a geração da 0.7, com o mesmo hash e o mesmo número (ref/10 §3.4.3).

E o `lab`:

```sh
$ jk agent desired --node lab
seq 2 · confirmed · luz 0.7: modo alert com o cpu_quente
```

Nunca recebeu a 0.7.1: o Deck não emitiu nada para a onda 2 (documento 20, D2). Desfaz o commit (`git revert HEAD`); o Deck vê que os dois nós já têm essas gerações como desejadas, e não emite nada. Corrigir a sério é outra versão (exercício 3).

## 6. Um nó offline

O protocolo foi desenhado para nós que desaparecem semanas (ref/10 §3.11.5). No `zero`, agenda o regresso do Wi-Fi antes de o cortar, porque o SSH vai com ele:

```sh
zero$ sudo systemd-run --on-active=40m /usr/sbin/rfkill unblock wifi
zero$ sudo rfkill block wifi
```

O LED do `zero` continua a piscar: o nó não precisa do Deck para nada (princípio 1). Com ele fora, faz dois commits seguidos no `joker.toml`: `half_period = "400 ms"` e, uns minutos depois, `"300 ms"`. São parâmetros, e por isso só mexem em manifestos (a app continua na 0.7).

```
12:10:40 zero     sem poll há 2m · pendente
12:11:05 rollout  01JAY1C8… (400 ms) · onda 1 canario: zero pendente (offline) · à espera
12:16:30 rollout  01JAY1C8… superseded · 01JAY1J3… (300 ms) · onda 1: zero pendente (offline) · à espera
12:49:12 zero     hello · desired_seq 4 · relatório completo · fila: 212 eventos
12:49:14 zero     seq 6 aceite · …confirmed 12:55:02 · onda 2 estavel: lab · seq 3
```

A onda 1 esperou, porque só passa com pelo menos um nó confirmado: um canário offline não autoriza nada. Quando o `zero` voltou, recebeu **só o estado de hoje**, o 300 ms. O `seq 5` (400 ms) nunca lhe chegou: o Deck guarda o último estado desejado de cada nó, e nunca uma fila (ref/10, D1). O relatório completo foi antes da fila de eventos acumulada, para o Deck saber o presente antes de digerir o passado.

```sh
$ jk agent status --node zero
autoridade   bancada-deck · ligado há 6 min · offline 39 min (12:10–12:49) · relógio synced
desejado     seq 6 · confirmed · fila 0 · perdidos 0
```

Receber o mesmo estado desejado duas vezes não faz nada; receber o de hoje depois de falhar três não faz três coisas. É isso que "estado desejado idempotente" quer dizer.

## 7. O que o Deck não pode fazer

Os princípios do documento 20, §2, vistos na tua bancada:

- **Não escreve em sinais.** Procura no `deck.toml`, no `fleet.toml` ou na ref/10: não há nenhuma mensagem de escrita (D9). O Deck mudou o pisca do `zero` com um commit, uma geração assinada e uma janela de confirmação, e o árbitro e o envelope do nó decidiram o resto. Uma escrita remota só vai existir dentro de uma sessão (1.3), pela API local, como a tua: com TTL, até ao `max_manual`, no registo de decisões.
- **Não lê segredos.** O `secrets.toml.enc` que ele lê do Git tem o `mqtt_bancada` cifrado para `node:lab`, `node:zero` e `user:dl`, e o Deck não tem nenhuma dessas chaves. O que os nós lhe reportam são nomes e revisões (`mqtt_bancada rev 1 ok`), nunca valores.
- **Não assina artefactos.** Viste-o na secção 3: sem o `jk publish`, `J0915`. Mesmo comprometido, com a chave `deck`, só consegue escolher entre gerações já assinadas por ti, dentro das janelas e do `max_applies_per_day` de cada nó; não consegue baixar para `dev` (`min_mode = prod`, `J0956`), nem fazer o nó aceitar uma app assinada por outra chave (`J0951`), nem mudar o bundle sem a raiz que está na tua pen (ref/10 §4, S1).

## 8. O que aconteceu por baixo

- **Duas camadas de assinatura** (ref/10, D2): o estado desejado é assinado pela chave `deck`, que vive no Deck; os artefactos pelas chaves `config`, `apps` e `secrets`, que não vivem. As ondas, os rollbacks e os tokens não precisam de cerimónia; o que corre no nó precisa de ti.
- **A política é do nó** (ref/02 §2.5.4): bundle ∩ `[security.trust]` − revogadas, com o `min_mode` do bundle e o `allow_dev` do `node.toml` como duas chaves para o `dev`.
- **Validação e aplicação** (ref/10 §3.3–§3.5): o nó repete tudo o que o Deck verificou (V1–V24), descarrega e prepara antes de trocar, troca num limite de tick, confirma por H1–H7, e volta atrás sozinho. O Deck não tem caminho especial: é o `jk apply --confirm` da parte 2 (ADR 0015), feito por outro.
- **Rollouts** (documento 20, §4.4; ref/10 S2): o Deck só decide *a quem* e *quando* emitir; parar é deixar de emitir, mais um `reject` para quem ainda confirma. O canário é o nó mais fraco porque é o que falha primeiro.
- **Offline e regresso** (ref/10 S3, §3.9): o `seq` dá a ordem, o `hello` traz o `desired_seq`, a fila de eventos é um cursor com `gap` quando enche, e o Deck guarda o último estado, não a história.
- **Alterações locais** (ref/10 §3.7.1, S12): com `local_changes = "hold"`, um `jk apply` teu no nó põe o Deck em espera (`J0966`) em vez de ser desfeito cinco minutos depois.

## 9. Exercícios

1. Com um rollout em curso, faz `jk apply --env lab-link --node lab --confirm 5m` com uma mudança local. O que diz o `jk agent status`? Leva a mesma mudança ao Git e vê a espera acabar sozinha (ref/10 S12).
2. Roda a palavra-passe: `mosquitto_passwd` no `lab` e `jk secret set mqtt_bancada` outra vez (rev 2). Quantas gerações novas houve? O que emitiu o Deck? (ref/02 §3.4.4)
3. Faz uma 0.7.2 que mantém a mediana sem o custo: um anel com a mediana calculada numa tarefa async a cada segundo, e um `Internal` para a tarefa de ciclo. Passa o canário?
4. Troca as coortes: `lab` canário, `zero` estável. Com a 0.7.1, até onde chegava o rollout, e que nó a recebia com os overruns?

```sh
$ git add -A && git commit -m "trilha(11): produção e frota"
```

**Estado no fim:** app `luz` 0.7 com `half_period = "300 ms"`; `mode = "prod"`; ficheiros novos `keys/*.pub`, `secrets.toml.enc`, `fleet.toml`; `[telemetry.uplink.bancada]`, `[security]` e `accept_missing_fallback` no `joker.toml`; `micro/esp/joker.toml` em `prod`, com `accept_unsafe_reset`; `[accepted]` nos locks (também no `micro/esp/joker.lock`); os dois nós enrolados em `bancada-deck`; fora do projeto, `~/deck/deck.toml` e a raiz numa pen.

---

## Fim da trilha

O que cada parte te deixou a saber, e onde ir mais fundo, está na tabela do fim do [README](README.md).

A linha que atravessa tudo: **nenhuma escrita contorna o envelope**, venha de uma app, de um MCU, de uma pessoa ou de um commit; e **cada camada fica segura sozinha**, do `safe` do LED à raiz na pen.

**O que te espera na 1.x** (documento 21, §2): ML, streams e pipelines, sinais em bloco, escritas agendadas e intenções (1.1); Structured Text, o SDK Python e mais protocolos (1.2); o Joker Studio, a operação remota e o multi-inquilino no Deck (1.3); Wasm nos MCUs (1.4); a observabilidade e a auditoria da frota e os MCUs autónomos em escala (1.5, LTS). Tudo isto por cima dos mesmos contratos que usaste aqui: o manifesto, o lock, a ref/10.
