# Part 11 — Production and fleet: signatures, secrets, the Deck and two nodes

**Goal:** put the project in `mode = "prod"`, with signatures and a secret, and start updating the two Raspberry Pis from Git, through the Deck, with a canary that stops a bad version before it reaches the other node.

**You will learn:** what `prod` changes · keys per role and the trust bundle · `accept_missing_fallback` and `max_manual` · secrets encrypted per recipient · the Deck MVP · enrollment with a token · desired state, waves and confirmation · `halt_if` and rollback · an offline node · what the Deck cannot do.

**Before you start:** parts 1–10. `lab` (Pi 3) runs `joker.lab-link.toml` (generation 15), with the ESP32 as Joker Micro over Wi-Fi (parts 9–10) and `luz` 0.6; the CA `~/org-ca` is the one from part 2 (`--org bancada`). `zero` has been stopped since part 7, at generation 2 of `joker.zero.toml` (`luz` 0.5): turn it on (`zero$ sudo systemctl start jokerd`). **The `esp` of `zero` becomes `bad(comm_failure)`**, and `esp_offline` active: since part 9, the ESP32's firmware is Joker Micro, which no longer serves Modbus TCP. It is the tolerant behavior of 0.4 (part 5): `zero` keeps blinking with its `ACT` LED. The laptop and the two Pis are on the same network.

---

## 1. Switch to `mode = "prod"`

In `dev`, Joker lets through what is not ready yet and warns. In `prod`, the same codes become errors and signatures become required (ref/01 §5.2). Change one line of `joker.toml`:

```toml
[project]
name  = "bancada"
joker = "1.0"
mode  = "prod"
```

### 1.1 What `jk check` says

```sh
$ jk check --env lab-link
```

```
error[J0205]: device fallback missing or inconsistent with `safe`
  --> joker.toml:6:1
   |
 6 | [outputs."pi.act_led"]
   | ^^^^^^^^^^^^^^^^^^^^^^ the `sysfs` driver has no fallback: if jokerd stops, the ACT LED stays as it was
   = help: on an output without physical consequences, justify it with accept_missing_fallback = "…" (ref/01 §5.10)

error[J0794]: node in `prod` with an MCU whose generation is `dev`
  --> micro/esp/joker.toml:5:9
   |
 5 | mode  = "dev"
   |         ^^^^^ the device esp (joker-link, joker.lab-link.toml) runs this manifest
   = note: a node in `prod` only sends `prod` generation blobs to an MCU (DEV bit at 0, ref/08 §2.16)
   = help: switch the MCU manifest to mode = "prod"
2 errors · in `dev` the first was the warning W0205, and the second did not exist
```

These are two problems, one on each side of the cable. The first is the warning you saw in part 2, now for real. An indicator LED has no physical consequences, and Joker accepts that if you say so in writing. Take the opportunity to limit manual writes on this output to 30 minutes:

```toml
[outputs."pi.act_led"]
safe                    = false
max_manual              = "30m"
accept_missing_fallback = "board ACT LED: indicator, no physical consequences"
```

**The MCU switches to `prod` too.** The `[project] mode` of `micro/esp/joker.toml` is the mode of the blob that `lab` takes to the ESP32, and it has been `dev` since part 9. Change it (`mode = "prod"`) and check again:

```sh
$ jk check --env lab-link
```

```
error[J0786]: board without safe outputs on reset (`reset_safe = false`); acceptable with `accept_unsafe_reset`
  --> micro/esp/joker.toml:8:9
   |
 8 | board = "esp32-devkit@1"
   |         ^^^^^^^^^^^^^^^^ until step 6 of boot, the pins stay in the hardware reset state
   = note: in `dev` it was the warning W0786 of part 9
   = help: if the site's hardware makes it acceptable, justify it with accept_unsafe_reset = "…" in [target] (ref/01 §5.22)
1 error
```

In `prod`, the MCU's checks are errors like the node's. The board `esp32-devkit` declares `reset_safe = false`: between the reset and the moment the firmware configures the pins, Joker guarantees nothing about the outputs. On the bench, you know what the hardware does: GPIO2 is an ESP32 strapping pin, pulled low, and the LED stays off at reset. Write it in the `[target]` of the MCU manifest:

```toml
# micro/esp/joker.toml ([project] and [target] change)
[project]
name  = "bancada-esp"
joker = "1.0"
mode  = "prod"

[target]
board               = "esp32-devkit@1"
accept_unsafe_reset = "bench: the GPIO2 LED is on a strapping pin pulled low, so it stays off at reset"
```

`J0786` becomes a warning. The acceptance does not change the firmware (the pins stay in the reset state until step 6, ref/08 §2.16): it only records who accepted it, why and for which board. The key in the lock is the resolved board, `esp32-devkit@1`: on another board, the acceptance does not hold (`J0440`).

Both justifications go to the locks, with who and when. There is one lock per input manifest, and the MCU has its own (`micro/esp/joker.lock`, part 10); so you do all three:

```sh
$ jk lock --env lab-link && jk lock --env zero && jk lock --manifest micro/esp/joker.toml
```

```toml
# joker.lab-link.lock (excerpt)
[accepted]
accept_missing_fallback."pi.act_led" = { reason = "board ACT LED: indicator, no physical consequences", by = "user:dl", at = "2026-09-25T09:12:40Z" }
```

```toml
# micro/esp/joker.lock (excerpt)
[accepted]
accept_unsafe_reset."esp32-devkit@1" = { reason = "bench: the GPIO2 LED is on a strapping pin pulled low, so it stays off at reset", by = "user:dl", at = "2026-09-25T09:13:05Z" }
```

The MCU's blob becomes a `prod` artifact too: without the `DEV` bit, and signed. What signs it is `jk publish`, with the key of the `config` role, like the generations of the Linux nodes (section 3; ref/02 §2.5.3). It is the end of the `W0779` of part 9: in `prod`, the firmware rejects an unsigned blob.

When this generation reaches the node (section 3), a manual write above the limit is rejected, and not silently shortened:

```sh
$ jk write pi.act_led true --ttl 2h --reason "test" --node lab
→ lab (prod) · from --node
error[J-E1102]: TTL above `max_manual`: pi.act_led accepts at most 30m
```

`zero` is at the `dev` level (document 23, §7), and that does not change with `prod`: `mode = "prod"` means "with production guarantees", and not "production hardware".

### 1.2 Keys, one per role

In `prod`, every artifact must come signed by a key with the right role (ref/02 §2.5.3). One key per role is what prevents the everyday key (the configuration one) from being the key to everything:

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

The private keys stay outside the project (`J0913` if they are inside); the `keys/` directory only has `.pub` files. The first four are signing keys (Ed25519): `config` signs generations, `apps` the apps, `segredos` the secret entries, `deck` the Deck's documents. The last one is your *escrow* key (X25519), the one that lets you read and re-encrypt secrets; tell `jk` where it is, with `secrets_key = "~/.local/share/joker/keys/dl.x25519.key"` in `[user]` of `~/.config/joker/config.toml` (ref/02 §2.3.2).

### 1.3 The trust bundle

File names give nobody a role. What says "this key signs apps" is the **trust bundle**, signed by the organization root, which is in the CA from part 2 (ref/10 §2.4.3):

```sh
$ jk ca bundle ~/org-ca --key config=keys/config.pub --key apps=keys/apps.pub \
    --key secrets=keys/segredos.pub --deck bancada-deck=keys/deck.pub \
    --deck-url https://portatil.local:8443 --min-mode prod --out ~/org-ca/trust.cose
```

```
trust bundle · org bancada · version 2 · signed by ed25519:5e0c9a17… (root)
  config   ed25519:2b7f41c0…   secrets  ed25519:4d0a6e2c…
  apps     ed25519:91c4e0a7…   deck     ed25519:e6f39b0c… (bancada-deck)
  authority bancada-deck · deck_urls https://portatil.local:8443 · min_mode prod
```

The root is only for this (and for changing anchors). Take it off the laptop: `mv ~/org-ca/root.key` to a USB stick, and put the stick away. The Deck never reads it (document 20, §4.4.2).

### 1.4 The manifest only restricts

```toml
[security]
trust = { config = ["keys/config.pub"], apps = ["keys/apps.pub"], secrets = ["keys/segredos.pub"] }

[security.roles]
operator = ["cert:CN=admin-*"]
admin    = ["cert:CN=admin-*"]
```

`[security.trust]` says "in this project, only these", and it can never give a key a role that the bundle did not give it (`J0292`; you see the error in section 3). `[security.roles]` is needed because in `prod` the defaults are the restrictive ones: without it, your user certificate from part 2 could only read (ref/01 §5.20).

## 2. Secrets: telemetry to `mosquitto`

Version 0.7 of the project publishes the temperature and the alarms over MQTT to a `mosquitto` on the Pi 3. What publishes is the **node**, through an uplink in the manifest, and not the app: a Wasm app does not open sockets. In `joker.toml` (ref/01 §5.17.3):

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
error[J0296]: connection without TLS or without security (`mqtt://`, `http://`, OPC UA `None`)
  --> joker.toml:61:12
   |
61 | broker   = "mqtt://lab.local:1883"
   |            ^^^^^^^^^^^^^^^^^^^^^^^ in `prod`, the password would go over the network in clear text
   = help: use mqtts:// (port 8883)
```

Change to `broker = "mqtts://lab.local:8883"`. `mosquitto` is not part of Joker; its certificate is made with `openssl`, through your CA's issuer, which the nodes already know:

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

The password you chose in `mosquitto_passwd` is the secret. It enters the project without ever being in clear text (ref/02 §2.4):

```sh
$ jk secret recipient add --user dl --key keys/dl.x25519.pub
$ jk secret set mqtt_bancada --description "lab's mosquitto" --sign-key ~/.local/share/joker/keys/segredos.key
value: ••••••••••••  (again) ••••••••••••
$ jk secret recipient add --node lab
$ jk secret recipient add --node zero
$ jk secret list
```

```
mqtt_bancada  rev 1  string  nodes: all  signer ed25519:4d0a6e2c…  recipients: node:lab, node:zero, user:dl
```

The value is never a command-line argument (it would stay in the shell history). `recipient add --node` gets the node's KEM key **from its certificate**, verified by your CA (`J0929` if it is not attested): nobody can make you encrypt for a key that is not the node's. `secrets.toml.enc` goes into Git (ADR 0024): it has the names and the recipients in clear text, and the values encrypted for three keys that the Deck does not have.

```sh
$ git add -A && git commit -m "bancada: prod, keys, MQTT telemetry with a secret"
```

## 3. The Deck on the laptop

The Deck is another binary, with its own `deck.toml`. The example of document 20, §4.4.2, is exactly that of this bench: copy it to `~/deck/deck.toml`, outside the project. It has the `id` `bancada-deck` and the `url` you put in the bundle, the project repository (`[git] repo = "~/joker-bancada"`, branch `main`, read every 30 s), the CA from part 2 (`[ca] dir = "~/org-ca"`), SQLite and the embedded store (in production, PostgreSQL and an OCI registry), and in `[signing]` the key `deck`. It is the only private key the Deck has: in a real installation, the Deck runs on another machine, and the keys `config`, `apps` and `segredos` never get there.

In the project repository, `fleet.toml` says which node runs which manifest and in which cohort. `zero` is the **canary**: the weakest node is the one that receives first.

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
bancada-deck · https://portatil.local:8443 · sqlite:deck.db · embedded store · deck key ed25519:e6f39b0c…
git    main 3c8e1f2 "bancada: fleet.toml" · 2 nodes, 2 cohorts, 2 waves
lab    generation sha256:61f0… · error[J0915]: unsigned artifact or generation in `prod` · nothing issued
zero   generation sha256:a3d7… · error[J0915]: unsigned artifact or generation in `prod` · nothing issued
```

This is the "what the Deck says" moment. The Deck validated both generations like `jk check`, and stopped where it had to stop: it does not have the `config` key, so it cannot sign what it read from Git. The one who signs is you, with your keys, and you upload to the Deck's store:

```sh
$ jk publish --env lab-link --env zero --registry oci://portatil.local:8443/bancada \
    --sign-key config=~/.local/share/joker/keys/config.key --sign-key apps=~/.local/share/joker/keys/apps.key
```

```
joker.lab-link.toml  generation sha256:61f0…  config ✓ · luz 0.6.0 apps ✓ · status_light_ctl 1.0.0 apps ✓ · MCU blob esp (prod) config ✓ · 7 artifacts uploaded
joker.zero.toml      generation sha256:a3d7…  config ✓ · 2 new artifacts (the others were already there, by digest)
```

The MCU's blob (`micro/esp/joker.toml`, now in `prod`) is signed here, with the same `config` key as the generation of `lab`, which takes it to the ESP32 over the link; the ESP32 verifies it against the `config` role of its bundle before opening it. The generation hash is the same in `jk` and in the Deck, because it is the same calculation with the same crates (ref/10 §2.6.2).

### 3.1 Enroll the nodes with a token

```sh
$ joker-deck token create --node lab --ttl 1d
jkt1_bancada_5e0c9a17d2b84f6e91a3c07d58e2f4b1_K7Q2M9XW4TPA6RZC3HNV8JDF5LBY1SEG  (lab · expires 2026-09-26T10:05Z · single use)
$ jk node enroll ssh://pi@lab.local --deck https://portatil.local:8443 --token jkt1_bancada_5e0c9a17… --force
```

```
→ ssh://pi@lab.local · already enrolled with ~/org-ca: --force generates a new identity
warning[J0933]: identity key in a file, without a TPM or secure element (or required attestation missing)
certificate CN=lab, O=bancada · until 2027-09-25 · issued by bancada-deck
anchors: ed25519:5e0c9a17… (matches the token) · bundle version 2 · authority bancada-deck
policy.allow_dev = false
```

The token carries the root's `kid`: that is how the node verifies the bundle it receives without trusting the network (ref/10 §2.5.1). It is the node that connects to the Deck, and the key never leaves it. Do the same for `zero`, with another token. The Pi 3 has no TPM, and `J0933` stays visible on each node with the key in a file (document 23, §5.4).

```sh
$ jk agent status --node lab
authority    bancada-deck · https://portatil.local:8443 · https, long-poll · connected for 40 s · skew 9 ms
desired      none accepted yet (seq 0) · queue 0 · paused: no · local changes: hold
```

And the Deck's log:

```
lab    enrolled · hello agent/1.0 · full report
lab    error[J0922]: secret not encrypted for this node: mqtt_bancada was encrypted for another certificate · on hold
```

`--force` gave `lab` a new identity, and with it a new KEM key (ref/02 §3.2.3). The secret was encrypted for the old one. The Deck cannot read it, but it sees that the `wrap` is for a certificate that is no longer the node's, and it does not send a generation that would fail on the node. The fix is the usual one:

```sh
$ jk secret recipient add --node lab && jk secret recipient add --node zero
$ git commit -am "bancada: secrets for the new identities" && jk check --env zero --node zero
```

`jk check --node` already knows the node's bundle, and this is where `J0292` would appear if `[security.trust]` put, for example, `keys/deck.pub` in the `apps` list: "it has only the role `deck` in the bundle". The manifest does not widen trust.

The Deck sees the commit and does the first rollout: today's generation, now in `prod`, first on `zero` and then on `lab`, through the waves (section 4). From here on both nodes reject `dev` generations (the bundle says `min_mode = prod`): `jk dev --node lab` answers `J0906`. The fast development loop now lives in the simulator, and the fleet hardware only changes through commits.

## 4. A good rollout: 0.7

0.7 adds a mode: with the alarm `cpu_quente` active, the light switches to `alert`.

```rust
// apps/luz/src/lib.rs — what changes from 0.6 to 0.7
#[joker::app(name = "luz", version = "0.7.0")]
pub struct Luz {
    // … the ports of 0.6, unchanged …
    #[input] quente: Input<bool>,        // alarm.cpu_quente.active
}

    // in the task `pisca`, right after `if !st.ligado { … }`:
    if io.quente.get_or(false) {
        io.sinal.alert();
        return Ok(());
    }
```

In `joker.toml`, the `bind` of `luz` gains `quente = "alarm.cpu_quente.active"` (every alarm creates that signal, ref/01 §5.15). Then, the path of a new version in `prod`:

```sh
$ jk test                                                   # the scenarios, including falha_esp.scenario.toml
$ jk bench luz --env lab-link --node lab          # the app changed digest: the old WCET is J0431
$ jk bench luz --env zero --node zero
$ jk check --env lab-link && jk check --env zero
$ jk publish --env lab-link --env zero --registry oci://portatil.local:8443/bancada \
    --sign-key config=~/.local/share/joker/keys/config.key --sign-key apps=~/.local/share/joker/keys/apps.key
$ git commit -am "luz 0.7: alert mode with cpu_quente"
```

In the Deck's terminal:

```
10:02:11 git      main a41c0de "luz 0.7: alert mode with cpu_quente"
10:02:12 rollout  01JAXQ7M… · lab sha256:7d2e… ✓ · zero sha256:c0a9… ✓ · 2 waves
10:02:12 wave 1   canario: zero · seq 2
10:03:40 zero     applying → confirming · window 15m · stable_for 5m
10:08:52 zero     confirmed · overruns_rate 0 · trips_rate 0 · rollbacks 0
10:18:52 wave 1   passed (10m without halt_if) · wave 2 estavel: lab · seq 2
10:25:30 lab      confirmed · rollout completed
```

On `zero`, while it confirms:

```sh
$ jk agent desired --node zero
seq 2 · rollout 01JAXQ7M… wave 1 (canario) · luz 0.7: alert mode with cpu_quente
phase generation · confirming · 3m12s of 5m consecutive healthy · window until 10:18:40
health  H1 groups ok · H2 apps ok · H3 no trips · H4 devices as before · H5 no alarms ≥ critical · H7 report-ack ✓
```

`auto` confirms itself through health **and** through an authenticated exchange with the Deck after the swap (H7): a generation that cut the node off the network would never confirm itself (ADR 0015, ref/10 §3.5). The `esp` of `zero` was already down before the swap, and H4 only compares with what was `up`.

Now watch 0.7 do what it says, on `lab`:

```sh
lab$ stress-ng --cpu 4 --timeout 5m
```

The 10 s average passes 70 °C, `cpu_quente` becomes active 10 s later, and the `ACT` LED switches to the alert pattern. On `mosquitto`, `mosquitto_sub -h lab.local -p 8883 --cafile ~/org-ca/ca.crt -u bancada -P … -t 'joker/#'` shows the alarm transition.

## 5. A bad rollout: 0.7.1

0.7.1 is an "improvement": instead of the `calc` average, a median of the last 2,048 ticks (41 s), computed in the cycle task.

```rust
fn mediana(janela: &mut Vec<f64>, x: f64) -> f64 {
    if janela.len() == 2048 { janela.remove(0); }   // shifts 2,047 values
    janela.push(x);
    let mut v = janela.clone();                      // 16 KiB copied on every tick
    v.sort_by(f64::total_cmp);                       // ~22,000 comparisons, every 20 ms
    v[v.len() / 2]
}
```

The tests pass (on the laptop, the cost cannot be seen). `jk bench` measures it:

```
lab   luz.pisca  p99.9 = 0.41 ms   max = 0.66 ms   rpi3b-plus
zero  luz.pisca  p99.9 = 12.6 ms   max = 15.3 ms   rpi-zero-w
```

```sh
$ jk check --env zero
RT admission (M23): cpu 0 · main 20 ms · 63% (limit 70%) · ok
warning[J0352]: group ≥ 10 ms without PREEMPT_RT: jitter not guaranteed
```

On the Zero, without Cranelift, Wasm is interpreted (document 23, §5.2), and the task goes from 0.4 to 12.6 ms. Admission accepts it: 63% fits in the schedulability test. But `bench` measures the task alone, and on the Zero there is one CPU core for everything: the historian, the TLS uplink, the Wi-Fi, the retries to the `esp` that does not answer. `jk publish`, commit, and the Deck starts with the canary:

```
11:40:05 wave 1   canario: zero · seq 3
11:40:31 zero     applying → confirming
11:41:02 zero     overruns_rate 0.061 (94 of 1,550 ticks) · halt_if "overruns_rate > 0.001"
11:41:02 rollout  01JAXW2P… halted · wave 2 not issued · reject → zero (seq 3, generation)
11:41:04 zero     rolled_back · error[J0969]: generation rejected by the Deck during confirmation: rollback
11:41:04 zero     seq 4: the previous generation (sha256:c0a9…), already reactivated · phase skipped
```

On `zero`, between 11:40:31 and 11:41:04, the LED blinks in jerks (each overrun skips a tick) and `jk status --node zero` shows `main 20 ms overrun · overruns 94` and the alarm `joker.group.main.overrun`. H1 never stayed 5 minutes in a row: without the Deck, the window would expire at 11:55:31 and the node would roll back anyway, with `J0961`. The Deck was just faster. The rollback **reactivates** the 0.7 generation, with the same hash and the same number (ref/10 §3.4.3).

And `lab`:

```sh
$ jk agent desired --node lab
seq 2 · confirmed · luz 0.7: alert mode with cpu_quente
```

It never received 0.7.1: the Deck issued nothing for wave 2 (document 20, D2). Undo the commit (`git revert HEAD`); the Deck sees that both nodes already have those generations as desired, and issues nothing. Fixing it for real is another version (exercise 3).

## 6. An offline node

The protocol was designed for nodes that disappear for weeks (ref/10 §3.11.5). On `zero`, schedule the Wi-Fi's return before cutting it, because SSH goes with it:

```sh
zero$ sudo systemd-run --on-active=40m /usr/sbin/rfkill unblock wifi
zero$ sudo rfkill block wifi
```

`zero`'s LED keeps blinking: the node does not need the Deck for anything (principle 1). With it away, make two commits in a row to `joker.toml`: `half_period = "400 ms"` and, a few minutes later, `"300 ms"`. They are parameters, so they only touch manifests (the app stays at 0.7).

```
12:10:40 zero     no poll for 2m · pending
12:11:05 rollout  01JAY1C8… (400 ms) · wave 1 canario: zero pending (offline) · waiting
12:16:30 rollout  01JAY1C8… superseded · 01JAY1J3… (300 ms) · wave 1: zero pending (offline) · waiting
12:49:12 zero     hello · desired_seq 4 · full report · queue: 212 events
12:49:14 zero     seq 6 accepted · …confirmed 12:55:02 · wave 2 estavel: lab · seq 3
```

Wave 1 waited, because it only passes with at least one confirmed node: an offline canary authorizes nothing. When `zero` came back, it received **only today's state**, the 300 ms. `seq 5` (400 ms) never reached it: the Deck keeps the last desired state of each node, and never a queue (ref/10, D1). The full report came before the accumulated event queue, so that the Deck knows the present before digesting the past.

```sh
$ jk agent status --node zero
authority    bancada-deck · connected for 6 min · offline 39 min (12:10–12:49) · clock synced
desired      seq 6 · confirmed · queue 0 · lost 0
```

Receiving the same desired state twice does nothing; receiving today's after missing three does not do three things. That is what "idempotent desired state" means.

## 7. What the Deck cannot do

The principles of document 20, §2, seen on your bench:

- **It does not write to signals.** Look in `deck.toml`, in `fleet.toml` or in ref/10: there is no write message (D9). The Deck changed `zero`'s blink with a commit, a signed generation and a confirmation window, and the node's arbiter and envelope decided the rest. A remote write will only exist inside a session (1.3), through the local API, like yours: with a TTL, up to `max_manual`, in the decision log.
- **It does not read secrets.** The `secrets.toml.enc` it reads from Git has `mqtt_bancada` encrypted for `node:lab`, `node:zero` and `user:dl`, and the Deck has none of those keys. What the nodes report to it are names and revisions (`mqtt_bancada rev 1 ok`), never values.
- **It does not sign artifacts.** You saw it in section 3: without `jk publish`, `J0915`. Even compromised, with the `deck` key, it can only choose among generations already signed by you, within the windows and the `max_applies_per_day` of each node; it cannot downgrade to `dev` (`min_mode = prod`, `J0956`), nor make the node accept an app signed by another key (`J0951`), nor change the bundle without the root that is on your USB stick (ref/10 §4, S1).

## 8. What happened underneath

- **Two layers of signatures** (ref/10, D2): the desired state is signed by the `deck` key, which lives in the Deck; the artifacts by the `config`, `apps` and `secrets` keys, which do not. Waves, rollbacks and tokens need no ceremony; what runs on the node needs you.
- **The policy belongs to the node** (ref/02 §2.5.4): bundle ∩ `[security.trust]` − revoked, with the bundle's `min_mode` and the `allow_dev` of `node.toml` as two keys for `dev`.
- **Validation and application** (ref/10 §3.3–§3.5): the node repeats everything the Deck checked (V1–V24), downloads and prepares before swapping, swaps at a tick boundary, confirms through H1–H7, and rolls back by itself. The Deck has no special path: it is the `jk apply --confirm` of part 2 (ADR 0015), done by someone else.
- **Rollouts** (document 20, §4.4; ref/10 S2): the Deck only decides *to whom* and *when* to issue; stopping is ceasing to issue, plus a `reject` for whoever is still confirming. The canary is the weakest node because it is the one that fails first.
- **Offline and return** (ref/10 S3, §3.9): `seq` gives the order, `hello` brings the `desired_seq`, the event queue is a cursor with a `gap` when it fills up, and the Deck keeps the last state, not the history.
- **Local changes** (ref/10 §3.7.1, S12): with `local_changes = "hold"`, a `jk apply` of yours on the node puts the Deck on hold (`J0966`) instead of being undone five minutes later.

## 9. Exercises

1. With a rollout in progress, do `jk apply --env lab-link --node lab --confirm 5m` with a local change. What does `jk agent status` say? Take the same change to Git and watch the hold end by itself (ref/10 S12).
2. Rotate the password: `mosquitto_passwd` on `lab` and `jk secret set mqtt_bancada` again (rev 2). How many new generations were there? What did the Deck issue? (ref/02 §3.4.4)
3. Make a 0.7.2 that keeps the median without the cost: a ring with the median computed in an async task every second, and an `Internal` for the cycle task. Does it pass the canary?
4. Swap the cohorts: `lab` canary, `zero` stable. With 0.7.1, how far would the rollout get, and which node would receive it with the overruns?

```sh
$ git add -A && git commit -m "track(11): production and fleet"
```

**State at the end:** app `luz` 0.7 with `half_period = "300 ms"`; `mode = "prod"`; new files `keys/*.pub`, `secrets.toml.enc`, `fleet.toml`; `[telemetry.uplink.bancada]`, `[security]` and `accept_missing_fallback` in `joker.toml`; `micro/esp/joker.toml` in `prod`, with `accept_unsafe_reset`; `[accepted]` in the locks (also in `micro/esp/joker.lock`); both nodes enrolled in `bancada-deck`; outside the project, `~/deck/deck.toml` and the root on a USB stick.

---

## End of the track

What each part left you knowing, and where to go deeper, is in the table at the end of the [README](README.md).

The thread that runs through everything: **no write bypasses the envelope**, whether it comes from an app, an MCU, a person or a commit; and **each layer stays safe on its own**, from the LED's `safe` to the root on the USB stick.

**What awaits you in 1.x** (document 21, §2): ML, streams and pipelines, block signals, scheduled writes and intents (1.1); Structured Text, the Python SDK and more protocols (1.2); Joker Studio, remote operation and multi-tenancy in the Deck (1.3); Wasm on MCUs (1.4); fleet observability and audit and standalone MCUs at scale (1.5, LTS). All of this on top of the same contracts you used here: the manifest, the lock, ref/10.
