# 15 — Mapa de ficheiros e vocabulário

Um índice de todos os ficheiros que um projeto Joker tem, quem os escreve, quando e onde estão definidos. A coluna "Definido em" aponta para o documento que explica e para a referência de `docs/ref/` que fecha o formato. E, antes disso, o vocabulário que costuma confundir: "dispositivo" quer dizer uma coisa precisa, e não é nem a placa nem a classe.

---

## 1. Vocabulário: placa, dispositivo, perfil, instância, classe

Na trilha há quatro "coisas físicas" diferentes, e cada uma tem o seu nome:

| Termo | O que é | Exemplo na trilha | Onde se declara |
|---|---|---|---|
| **Placa** (*board*) | o modelo de computador onde o Joker corre | o Raspberry Pi 3 B+ | `[target] board = "rpi3b-plus@1"` no manifesto; descrita por um **perfil de placa** |
| **Nó** (*node*) | uma placa concreta com `jokerd`, identificada e registada | `lab` (`lab.local`), o Pi 3 da bancada | `jk node enroll`; escolhido com `--node` na CLI |
| **Barramento** (*bus*) | um meio de comunicação partilhado | a porta série da ponte USB do ESP32 (`[buses.esp32]`) | `[buses.<nome>]` no manifesto |
| **Dispositivo** (*device*) | **uma coisa com que o Joker fala**, através de um driver | o `pi` (os LEDs e a temperatura, pelo `sysfs`), o `esp` (o ESP32 em Modbus RTU, unidade 1) | `[devices.<nome>]` no manifesto |
| **Perfil** (*profile*) | a descrição de um **modelo** de dispositivo: registos, escalas, unidades, fallback | `local/bench-esp32@1` | um ficheiro por modelo, em `profiles/` ou no registo |
| **Sinal** | uma grandeza de um dispositivo (ou calculada) | `pi.cpu_temp`, `esp.led` | no perfil, no modelo de simulação ou no manifesto |
| **Classe** | um **tipo** de equipamento, para os programas | `temp_sensor@1`, `led@1` | um ficheiro por classe, em `classes/` ou no registo |
| **Instância** | um equipamento concreto que cumpre uma classe | `cpu` (um `temp_sensor@1`), `led_pi`, `sinal` (uma `status_light@1` virtual) | implícita (perfil), no manifesto (`[instances.*]`) ou virtual (app) |

Duas relações que é preciso ter claras:

- **Dispositivo ≠ instância.** O dispositivo é *com quem se fala* (endereço, driver, protocolo). A instância é *o que aquilo é para o programa*. Às vezes coincidem: o ESP32 da trilha é o dispositivo `esp` **e**, pelo perfil, a instância implícita `esp` (um `std/led@1`, um `std/button@1` e um `std/analog_in@1`). Outras vezes não: o LED `ACT` é um sinal do dispositivo `pi` (o `sysfs`), e a instância `led_pi` foi composta no manifesto (trilha, parte 7). Uma bomba real é uma instância feita de sinais de três dispositivos.
- **Não há um ficheiro por dispositivo.** Os dispositivos são secções do manifesto. Os ficheiros são por **modelo**: um perfil serve todos os dispositivos daquele modelo (dez medidores iguais = um perfil), e um perfil de placa serve todas as placas daquele modelo.

---

## 2. Os ficheiros de um projeto

**O tipo está no nome** (ADR 0026). Cada ficheiro TOML escreve-se `<nome>.<tipo>.toml` (`.profile.toml`, `.board.toml`, `.sim.toml`, `.scenario.toml`, `.conformance.toml`, `.fixture.toml`, `.driver.toml`, `.package.toml`); as camadas do manifesto chamam-se `joker.toml` ou `joker.<variante>.toml`, em qualquer pasta, e os locks `joker.lock` ou `joker.<variante>.lock`. O conteúdo tem de bater com o sufixo (`J0803`, ref/03 §2.1.6). Os ficheiros de configuração da `jk`, do nó e do Deck têm nomes fixos (`config.toml`, `node.toml`, `deck.toml`, `fleet.toml`). A tabela completa está no ref/00 §2.

### 2.1 Escritos por pessoas

| Ficheiro | Formato | O que descreve | Quem escreve | Quando se mexe | Definido em |
|---|---|---|---|---|---|
| `joker.toml` | TOML | **o site**: placa, barramentos, dispositivos, saídas e envelopes, ciclos, apps e ligações, instâncias, modelos, pipelines, `calc`, alarmes, telemetria, pontes, segurança | integrador / engenheiro do site | sempre; é o ficheiro principal | 03, ref/01 |
| overlays `joker.<variante>.toml` (`joker.sim.toml`, `joker.rpi.toml`, `templates/joker.bess-v2.toml`, `sites/lisboa-01/joker.verao.toml`) | TOML, com `extends` | variações do manifesto: simulador vs hardware, template partilhado vs site concreto; escolhem-se com `--env <variante>` | integrador | quando o mesmo sistema tem mais do que um ambiente ou site | 03 §2, ref/01 §7 |
| `profiles/<fabricante>/<modelo>@<major>.profile.toml` | TOML | um **modelo de dispositivo**: registos, tipos, escalas, unidades, limites, enums, coerência, fallback, ações, `implements` | integrador de hardware | quando aparece um equipamento que ainda não está no registo | 05 §6, ref/03 §2.2 |
| `boards/<placa>.board.toml` | TOML | um **modelo de placa**, com `version`: núcleos recomendados, watchdog, IO fixo (com `fallback`), portas série, aceleradores; num MCU, flash, FPU, capacidades e sonda | quem suporta uma placa nova (raro) | quase nunca: vêm do registo | 05 §11, ref/03 §2.3 |
| `classes/<pacote>/<nome>.jkc` | `.jkc` (documento 17) | uma **classe**: membros, capacidades, métodos, ações, eventos, consultas, saúde | autor de abstrações | quando se cria um tipo de equipamento novo | 17, ref/03 §2.7 |
| `classes/<pacote>/<pacote>.package.toml` | TOML | um **pacote de classes**: nome, licença, autores, dependências (o pacote `local` não precisa dele) | autor de abstrações | quando se publica um pacote | 17 §3.2, ref/03 §2.7 |
| `classes/<pacote>/<nome>.conformance.toml` | TOML | os testes que qualquer implementação da classe tem de passar (o esquema único de conformidade) | autor de abstrações | com a classe | 14 §7.3, ref/03 §2.6 |
| `classes/<pacote>/<nome>.sim.toml` | TOML | o **simulador** de uma classe, para correr a conformidade e as apps sem hardware | autor de abstrações | opcional, com a classe | ref/03 §2.4.11 |
| `sim/<modelo>.sim.toml` | TOML | um **dispositivo simulado**: sinais, geradores, dinâmica, falhas, painel, `implements` | quem desenvolve | enquanto não há hardware, e para testes | 04 §6, ref/03 §2.4 |
| `sim/scenarios/<nome>.scenario.toml` | TOML | um **cenário de teste do site**: ações no tempo e expectativas | quem desenvolve | com cada comportamento novo | 04 §6, ref/03 §2.5 |
| `apps/<app>/` (`Cargo.toml` + `src/`, ou `package.json`, ou `pyproject.toml`) | código | uma **app**; as portas, as tarefas e os parâmetros são declarados no código, sem `app.toml` | programador | sempre | 04, ref/04, ref/05 |
| `keys/*.pub` | chaves públicas | chaves que o projeto usa: âncoras e chaves por papel (`runtime`, `config`, `apps`, `drivers`, `secrets`…); o manifesto só **restringe** o bundle de confiança do nó | responsável de segurança | no início, e em rotações | 03 §13, ref/02 §2.5 |
| `.joker/config.toml` | TOML | configuração local do cliente para o projeto: nó e manifesto (ou variante) por omissão (não versionado; o do utilizador está em `~/.config/joker/config.toml`) | quem desenvolve | raramente | ref/02 §2.3 |
| `.taplo.toml` | TOML | associa cada sufixo (`*.profile.toml`, `joker*.toml`…) ao esquema JSON do seu tipo, para completação e erros no editor | `jk init`, `jk new`, `jk migrate` (versionado) | quase nunca | ref/03 §2.1.6 |

### 2.2 Gerados, mas versionados

| Ficheiro | Gerado por | O que fixa | Versionar? |
|---|---|---|---|
| `joker.lock` e `joker.<variante>.lock` | `jk lock`, `jk bench`, `jk build` (digests de apps locais) | versões exatas de placas, perfis, classes, apps e drivers; mapas descobertos (SunSpec), firmware, fallbacks verificados, WCET medido, exceções aprovadas (`widen`, `accept_missing_fallback`) | **sim**: é o que torna o site reproduzível e deteta a deriva (ref/02 §2.1) |
| `secrets.toml.enc` | `jk secret` | os segredos do projeto, cada um cifrado para os nós destinatários e para as pessoas de escrow, assinado e com `rev` | **sim** (ADR 0024): só contém valores cifrados, e é assim que o Deck os distribui sem os ler (ref/02 §2.4) |

**Um lock por manifesto de entrada**, na mesma pasta: `joker.lock` para o `joker.toml` e `joker.<variante>.lock` para cada overlay de entrada (`joker.rpi.toml` → `joker.rpi.lock`, que é o lock de `jk <comando> --env rpi`; `sites/lisboa-01/joker.toml` → `sites/lisboa-01/joker.lock`). Um overlay usado só através de `extends` (um template) não tem lock (ref/02 §2.1.1).

### 2.3 Artefactos (produzidos, distribuídos, não editados)

Cada artefacto distribuído tem um tipo de media, e os tipos estão numa só tabela, com o papel de assinatura de cada um: ref/00, §7.1.

| Artefacto | Produzido por | Conteúdo | Tipo de media | Definido em |
|---|---|---|---|---|
| `*.wasm` | `jk build` | o componente de uma app, com a secção `joker:meta` | `joker.app.v1+wasm` | 04 §2, ref/04 §5 |
| `*.cwasm` | `jk build --target` ou instalação | o AOT para uma arquitetura | `joker.app.v1+cwasm` | 01 §9 |
| driver tier W (`*.wasm`) ou tier X (índice multi-plataforma) | build do driver | um driver, com o seu descritor | `joker.driver.v1+wasm`, `joker.driver.v1+tar` | ref/06 §4.7, §5.9 |
| `*.jktrace` | `jk record`, `jk trace-dump` | um traço reproduzível | `joker.trace.v1` | 16 §7 |
| firmware do Joker Micro (ELF/UF2/bin) | `jk build --target mcu:…` (`--generic` para o genérico) | runtime + apps + drivers para um MCU | `joker.micro-firmware.v1` | 19 §4, ref/08 §2.12 |
| `firmware.jkfw` | `jk build --target mcu:… --generic` | descritor do firmware genérico: placa, capacidades, formato e ISA que aceita | (com o firmware) | ref/08 §2.14 |
| `<site>.jkgen` | `jk build --target mcu:… --generation --firmware <f.jkfw>` | o blob de geração de um MCU, compilado e assinado (papel `config`) | `joker.generation.v1` | 19 §4.1, ref/08 §2.13 |
| `libjoker_node.a`, `joker_node.h`, `joker_project.h` | `jk build --target mcu:… --lib` | o Joker como biblioteca C: o header fixo da ABI e o header gerado do projeto | — | 19 §4.2, ref/08 §2.2, §2.8 |
| crate `joker-project` | `jk build --target mcu:… --crate` | a interface tipada do projeto para firmware Rust | — | ref/08 §2.11.4 |
| `*.contract.json` | `jkc` | a forma canónica de uma classe; vai com cada geração para o nó | `joker.class.contract.v1+json` | 17 §6 |
| relatório de conformidade | `jk class verify --report`, `jk driver test --report` | o resultado assinado de uma suite, publicado ao lado do artefacto | `joker.conformance-report.v1+json` | ref/03 §2.8, ref/06 §6.5 |
| geração de um nó Linux | `jk apply` (enviada ao nó) ou `jk publish` (publicada no registo, para o Deck) | manifesto efetivo, lock, contratos e artefactos por digest, assinados pela `jk` com a chave do papel `config`. **O Deck não produz nem assina gerações**: não tem as chaves da organização, calcula o descritor a partir do Git e procura no registo a geração assinada com esse descritor (`J0915` se faltar; documento 20, §4.4.2) | `joker.config.v1+cbor` | ref/10 §2.6, 07 §3 |
| `*.jkm` | `jk model build` (1.1) | um modelo: `model.toml` (assinatura e guarda), `lineage.toml`, `signature.sig` (COSE_Sign1, ADR 0025), `payload/` | `joker.model.v1+jkm` (reservado) | 06 §2 |

### 2.4 Segredos

| Ficheiro | O que é | Regra |
|---|---|---|
| `secrets.toml.enc` | segredos (credenciais de brokers, APIs), cada um cifrado com HPKE para a chave KEM de cada nó destinatário e assinado | **versionável** (ADR 0024); o manifesto refere-os por nome (`secret:<nome>`), e as apps por um nome local em `[apps.<app>.secrets]` |
| ficheiro em claro para `jk secret import` | a entrada de uma importação | **nunca** em git; apaga-se depois de importar |
| `.joker/secrets.dev.toml` | segredos em claro só para a runtime local do `jk dev` | nunca em git (`.joker/` está no `.gitignore`) |
| chaves privadas de assinatura e de escrow | em `~/.local/share/joker/keys/`, num HSM ou `pkcs11:` | nunca no projeto (ref/02 §2.5.1) |

### 2.5 No dispositivo

**No dispositivo não se editam ficheiros.** Tudo chega por `jk apply` (configuração, artefactos e segredos), por `jk secret push`, pelo Deck (que distribui o que o `jk publish` assinou), ou por `jk node enroll` (identidade). O estado da runtime vive em `/var/lib/joker/` e é gerido pelo `jokerd`. Editar lá à mão é equivalente a um patch não versionado, e o `jk doctor` dá `J0907` se encontrar diferenças.

**A exceção é `/etc/joker/node.toml`**, a configuração de arranque do nó: só o que é preciso antes da primeira geração, o que tem de sobreviver a uma geração má (a escuta mínima da API, os administradores mínimos, as âncoras de confiança, a política de modos) e o que é da instalação física. Mesmo este ficheiro **não se edita à mão**: só o `jokerd` o escreve, no enrolamento e na troca de âncoras (`jk node trust set`). A geração só lhe acrescenta, nunca lhe retira (ref/02 §2.2).

| Onde | O quê |
|---|---|
| `/etc/joker/` | `node.toml`, `keys/*.pub` (âncoras), `enroll-token` opcional |
| `/run/joker/` | `api.sock`; `enroll-code` enquanto o nó não tem identidade |
| `/var/lib/joker/identity/`, `trust/` | certificado e chave do nó, chave KEM, bundle de confiança ativo |
| `/var/lib/joker/generations/<gen>/` | uma pasta por geração (documento assinado, manifesto efetivo, lock, contratos); `current` e `confirmed` são ligações |
| `/var/lib/joker/artifacts/`, `aot/` | armazém por conteúdo e AOT |
| `/var/lib/joker/secrets/` | segredos recebidos, ainda cifrados |
| `/var/lib/joker/state/`, `kv/`, `store/`, `trace/`, `link/` | trips, alarmes, intenções, `kv` das apps, historian e registo de decisões, anéis de traço, cursores de uplink |
| `/var/lib/joker/mcu/`, `runtime/a\|b/` | firmware e blobs em espera para os MCUs; slots A/B do `jokerd` |

A árvore completa, com dono e permissões de cada entrada, está na ref/02, §2.2.4.

### 2.6 No repositório do Joker: `wit/` e `spec/`

Não são ficheiros de um projeto, mas são os que definem as interfaces que um projeto usa. São parte das referências (ADR 0023): se um deles e o texto da referência divergem, é um bug nos dois.

| Ficheiro | O que é | Referência |
|---|---|---|
| `wit/joker-runtime.wit` | o mundo das apps (`joker:runtime`) | ref/04 |
| `wit/joker-driver.wit` | o mundo dos drivers tier W (`joker:driver`) | ref/06 §4 |
| `wit/joker-sim.wit` | as plantas simuladas em Wasm do driver `sim` | ref/03 §2.4.10 |
| `spec/cddl/joker-meta.cddl` | a secção `joker:meta` das apps | ref/04 §5 |
| `spec/cddl/joker-link.cddl` | as mensagens do Joker Link | ref/07 |
| `spec/cddl/joker-micro.cddl` | o blob `.jkgen` e o descritor `.jkfw` | ref/08 |
| `spec/cddl/jdp.cddl`, `spec/c/jdp.h` | o protocolo dos drivers tier X | ref/06 §5 |
| `spec/cddl/agent.cddl` | o protocolo do agente `agent/1` | ref/10 |
| `spec/cddl/secrets.cddl` | o conjunto de segredos selado para um nó | ref/02 §2.4 |
| `spec/openapi/joker-api.yaml` | a API HTTP local (OpenAPI 3.1) | ref/09 |
| `spec/c/joker_node.h`, `spec/c/example/` | a ABI C do Joker Micro e um exemplo com `joker_project.h` | ref/08 |

---

## 3. Quem toca em quê

A maioria das pessoas só mexe em três ou quatro destes ficheiros:

| Pessoa | Ficheiros |
|---|---|
| **Integrador do site** | `joker.toml` e overlays, os locks (via `jk lock`), `secrets.toml.enc` (via `jk secret`), às vezes um perfil |
| **Integrador de hardware** | `profiles/**`, raramente `boards/**` |
| **Autor de abstrações** | `classes/**/*.jkc` (definição + conformidade), `apps/<implementação>/` |
| **Programador de negócio** | `apps/<app>/`, `sim/scenarios/**`, às vezes `sim/*.sim.toml` |
| **Operador** | nenhum: usa `jk` (write com TTL, alarmes, intenções) |

---

## 4. Da pasta vazia ao sistema completo

O mesmo mapa aplicado à trilha de estudo (projeto `joker-bancada/`), parte a parte. A lista completa, com o que cada ficheiro é no fim, está no `trilha/README.md`, "A pasta, no fim da trilha".

| Parte | Ficheiros novos ou alterados |
|---|---|
| 1 | `joker.toml` (comum) e `joker.sim.toml` (overlay) desde o início, `joker.sim.lock`, `sim/pi.sim.toml`, `sim/scenarios/pisca.scenario.toml`, `apps/luz/` (0.1); fora do projeto, `.joker/config.toml` |
| 2 | `joker.lab.toml` (placa `rpi3b-plus@1`, `[resources]`, o `pi` pelo `sysfs`), `joker.lab.lock`; fora do projeto, `~/org-ca/` e o contexto `lab` em `~/.config/joker/config.toml` |
| 3 | `sim/pi.sim.toml` (`carga`, `cpu_temp`), `joker.toml` (`[calc]`, `[alarms.cpu_quente]`, `[telemetry.record]`), `joker.lab.toml` (`pi.cpu_temp`), `sim/scenarios/aquece.scenario.toml`, `apps/luz/` (0.2) |
| 4 | `mapa-esp32.csv`, `profiles/local/bench-esp32@1.profile.toml`, `sim/esp.sim.toml`, `joker.sim.toml` e `joker.lab.toml` (`[buses.esp32]` com `dtr`/`rts`, `[devices.esp]`), `joker.toml` (`[outputs."esp.led"]`, debounce), `joker.lab.lock` (perfil, `[fallback]`, `[discovered]`), `sim/scenarios/botao.scenario.toml`, `apps/luz/` (0.3) |
| 5 | `joker.toml` (`[signals."esp.touch"]`, `calc.toque_nivel`, `[alarms.esp_offline]`), `sim/esp.sim.toml` (tecla de falha), `sim/scenarios/falha_esp.scenario.toml`, `apps/luz/` (0.4 e 0.4.1), `joker.lab.lock` |
| 6 | `joker.rpi.toml` (o `[devices.pi]` sai do `joker.lab.toml`), `joker.zero.toml`, `joker.zero.lock`, `boards/rpi-zero-w.board.toml`; `joker.lab.toml` passa a estender `joker.rpi.toml` |
| 7 | `joker.toml` (cinco `[instances.*]` compostas, `bind` da `luz`), `profiles/local/bench-esp32@1.profile.toml` e `sim/esp.sim.toml` (`[implements]`), `apps/luz/` (0.5), os três locks |
| 8 | `classes/local/status_light.jkc` (+ `status_light.conformance.toml`), `apps/status_light_ctl/`, `joker.toml` (instância virtual `sinal`, as suas saídas, `order` do `main`), `apps/luz/` (0.6), `sim/scenarios/pisca.scenario.toml`, os três locks |
| 9 | `micro/esp/joker.toml`, `joker.lab-link.toml`, `joker.lab-link.lock`, `secrets.toml.enc` (destinatário `node:esp`) |
| 10 | `micro/esp/apps/reflexo/`, `micro/esp/joker.lock` (`[wcet]` do chip), `micro/esp/joker.toml` (`[net.wifi]`, `[link]` em UDP), `joker.lab-link.toml`, `secrets.toml.enc` (`wifi_bancada`) |
| 11 | `keys/*.pub`, `fleet.toml`, `secrets.toml.enc` (`mqtt_bancada`), `joker.toml` (`mode = "prod"`, `[telemetry.uplink]`, `[security]`, `accept_missing_fallback`), `[accepted]` nos locks, `apps/luz/` (0.7 e 0.7.1); fora do projeto, `~/deck/deck.toml` |

---

## 5. Porque são tantos, e porque não menos

Cada ficheiro existe porque tem **um dono diferente** ou **uma vida diferente**:

- o **perfil** é de um modelo de equipamento e reutiliza-se em todos os sites; se vivesse no manifesto, cada site teria a sua cópia dos registos do mesmo medidor;
- a **classe** é um contrato entre muitas apps e muitos perfis; não pertence a nenhum site;
- o **lock** muda quando o mundo muda (firmware, descoberta), e não quando alguém edita;
- os **cenários** são testes e mudam com o comportamento, e não com o hardware;
- os **overlays** separam o que o sistema faz (comum) de onde está (sim ou hardware, site A ou B).

Juntá-los reduziria o número de ficheiros e aumentaria o número de cópias. É a mesma separação que o Cargo faz entre `Cargo.toml`, `Cargo.lock` e o registo de crates.
