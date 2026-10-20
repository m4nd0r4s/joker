# 20 — A família de produtos e o Joker Deck

O Joker começou como uma runtime. Com o Joker Micro (documento 19) e com a pergunta "quem orquestra 3 000 destes?", passa a ser uma **família**. Este documento define as peças, os contratos entre elas e, com mais detalhe, a peça que falta: o **Deck**, o plano de controlo da frota.

É normativo quanto aos princípios e aos contratos (ADR 0021). O desenho interno do Deck é uma **proposta de arquitetura**: terá o seu próprio conjunto de documentos quando começar.

---

## 1. A família

O tema escreve-se sozinho: um joker é uma carta, e as cartas vivem num baralho.

| Peça | O que é | Onde corre | Estado |
|---|---|---|---|
| **Joker** (`jokerd`, `jk`) | a runtime em Linux | Raspberry Pi, RevPi, PCs industriais, Jetson | v1 |
| **Joker Micro** | a runtime em microcontroladores | RP2350, ESP32-C6/S3, STM32 | v1 |
| **Joker Hub** | o registo de artefactos: perfis, placas, classes, apps, (modelos na 1.1) | nuvem ou on-premises | v1 (mínimo), cresce depois |
| **Joker Deck** | o plano de controlo da frota: inventário, estado desejado, rollouts, observabilidade, acesso remoto | nuvem ou on-premises | MVP na v1, produto próprio depois |
| **Joker Studio** | a interface de comissionamento e desenvolvimento: painel, navegador de sinais, traço e replay, escritas manuais | local (`jk ui`) e dentro do Deck | v1.x |

```
                         ┌──────────────── Joker Deck ────────────────┐
  Git (sites, templates) │ inventário · estado desejado · rollouts ·  │ ◀── operadores, integradores
  ──────────────────────▶│ observabilidade · alarmes · acesso remoto  │
                         └───────┬───────────────────────▲────────────┘
                   estado desejado│ (assinado)           │ estado reportado, telemetria
                                  ▼                      │
            ┌──────── nó Linux (jokerd) ────────┐        │
            │ apps · árbitro · envelope · ...   │────────┘
            └──────┬─────────────────▲──────────┘
             Joker Link│             │SAMPLES/OUTCOMES          ┌── Joker Hub ──┐
                   ▼   │             │                          │ perfis, placas│
            ┌── Joker Micro ──┐    ┌── Joker Micro (autónomo) ──┐│ classes, apps │
            │ coprocessador   │    │ Wi-Fi → Deck diretamente   ││ (OCI)         │
            └─────────────────┘    └────────────────────────────┘└───────────────┘
```

---

## 2. Os princípios que não se negociam

1. **Local-first.** Um nó funciona sozinho, para sempre, sem Deck nem Hub. Tudo o que o Deck faz, a `jk` faz também, nó a nó. O Deck serve a escala, e não a sobrevivência.
2. **O Deck nunca está no caminho de controlo.** Não há nenhuma escrita em tempo real que dependa do Deck, e **o protocolo do agente não tem nenhuma mensagem de escrita em sinais**: o Deck muda configuração (gerações) e abre sessões. Uma escrita remota só existe dentro de uma sessão (1.3), pela API local do nó, como a de uma pessoa no local: escritor `manual` com o principal `deck:<deck_id>/user:<id>`, TTL, árbitro e envelope de sempre (ref/10, D9).
3. **Contratos abertos.** O protocolo do agente, o formato dos artefactos (OCI), a telemetria (Sparkplug B, Zenoh, OTLP) e a API do Hub são especificados publicamente. Qualquer um pode escrever outro Deck, ou ligar o Joker a Mender, balena ou a um SCADA.
4. **Git é a fonte da verdade da configuração.** O Deck lê templates, overlays e sites de um repositório (GitOps); edições na interface geram commits. Nada vive só na base de dados do Deck.
5. **A segurança do nó não depende do Deck.** As assinaturas e a política de confiança são verificadas no nó; um Deck comprometido não consegue fazer um nó aceitar um artefacto que as chaves do nó não aceitem.

---

## 3. Os contratos

### 3.1 Protocolo do agente (nó ↔ Deck)

A forma fechada é o protocolo `agent/1`, na ref/10 (mensagens, enrolamento, confiança, confirmação, transportes, perfil micro), com os esquemas em `spec/cddl/agent.cddl`. Em resumo:

| Mensagem | Sentido | Conteúdo |
|---|---|---|
| **estado desejado** | Deck → nó | documento assinado: versão da runtime, geração (hash de manifesto + lock + contratos), artefactos (apps, perfis, firmware de MCUs, modelos na 1.1) por digest OCI, janela de confirmação, coorte |
| **estado reportado** | nó → Deck | versões em execução, geração confirmada, deriva do lock, saúde (grupos, dispositivos, instâncias), alarmes ativos, trips, uso de recursos, MCUs ligados e as suas versões |
| **eventos** | nó → Deck | transições de alarmes, decisões marcadas para auditoria, resultados de aplicações de geração |
| **sessão remota** (1.3) | Deck ↔ nó | túnel aberto pelo nó, com TTL, para a `jk` e para o Studio, com papéis do nó e consentimento local |

- **Transporte:** HTTPS com long-poll, ou MQTT; tolera meses offline (o nó continua no último estado confirmado).
- **Aplicação:** o nó aplica o estado desejado com o mecanismo de sempre (documento 01, §15): validar, planear, trocar num limite de tick, esperar pela confirmação, rollback automático se a saúde falhar. O Deck não tem um caminho especial. Uma geração só se confirma sozinha depois de o Deck acusar um relatório (`report-ack`): uma geração que cortou a rede ao nó não se confirma a si própria (ref/10 §3.5).
- **MCUs:** um MCU atrás de um nó Linux recebe firmware e gerações pelo Joker Link, orquestrado pelo nó. Um MCU autónomo com rede fala o mesmo protocolo, numa versão reduzida (CBOR sobre MQTT ou CoAP).

### 3.2 Artefactos (Hub)

Tudo é um artefacto OCI assinado, com um tipo de media próprio e um papel de assinatura: a runtime, a geração de um nó Linux, apps, drivers, perfis, placas, classes (com o contrato e a conformidade), o firmware e o blob de geração dos MCUs, os segredos selados e, na 1.1, os modelos. A tabela única dos tipos, com a forma curta (`joker.app.v1+wasm`) e a forma completa do registo (`application/vnd.joker.app.v1+wasm`), está na ref/00, §7.1. Um registo OCI comum (Harbor, GHCR, ECR…) serve como Hub mínimo; o Hub acrescenta pesquisa, compatibilidade (que perfis implementam que classes) e as verificações de publicação (conformidade das classes, `jk class diff`).

### 3.3 Telemetria

O Deck consome o que os nós já enviam: Sparkplug B ou Zenoh para sinais e alarmes, OTLP para as métricas da própria runtime (documento 01, §13 e §19). Não há um formato "do Deck".

---

## 4. O Joker Deck, em detalhe

### 4.1 O que faz

| Área | Funcionalidades |
|---|---|
| **Inventário** | nós e MCUs, placas, versões, localização, cliente/site, estado do enrolamento, certificados |
| **Configuração** | sites a partir de templates + overlays (M13) num repositório Git; diff entre o desejado e o reportado; deteção de deriva |
| **Rollouts** | coortes e ondas (1% → 10% → 100%), paragem automática por KPI (taxa de alarmes, trips, overruns, rollback), janelas de manutenção por site |
| **Observabilidade** | saúde da frota, alarmes agregados, métricas da runtime, painéis por site |
| **Operação remota** | `jk` e Studio por túnel, escritas manuais e aprovação de intenções (M18) com auditoria |
| **Identidade** | CA para o enrolamento de nós e MCUs, rotação de certificados, revogação |
| **Auditoria** | quem mudou o quê, quando, em que site, com que aprovação; registos de decisões dos nós a pedido |
| **Multi-inquilino** | um integrador gere as frotas dos seus clientes; cada cliente vê só as suas |

### 4.2 Arquitetura proposta

- **Servidor** em Rust (a mesma linguagem, os mesmos crates de manifesto, lock, `jkc` e verificação que a `jk` usa: o Deck valida uma geração exatamente como o `jk check`).
- **Estado:** PostgreSQL para inventário, rollouts e auditoria; um registo OCI para artefactos; TimescaleDB ou equivalente opcional para métricas; o Git para configuração.
- **Interface web**, com o Studio embutido para cada nó.
- **Implantação:** SaaS ou on-premises, em containers; um Deck pequeno corre num único servidor para frotas de algumas centenas de nós.

### 4.3 Estresse

**D1 — 10 000 nós, 30% offline durante semanas.** O estado desejado é idempotente e assinado, e o nó aplica-o quando volta. O rollout conta um nó offline como "pendente", e não como falha. **Aguenta.**

**D2 — Um rollout mau.** A versão 3.2 da app aumenta os trips em 1% dos nós da primeira onda. Com uma regra `halt_if = "trips_rate > baseline * 1.5"`, o rollout para: o Deck deixa de emitir estados desejados para as ondas seguintes. Os nós afetados voltam atrás por janela de confirmação, por um comando `reject` (os que ainda estão a confirmar), ou por um estado desejado novo que aponta para a geração anterior, que o nó **reativa** com o mesmo hash e o mesmo número (ref/10 §3.5, S2). **Aguenta**, e é a razão para as ondas existirem.

**D3 — O Deck é comprometido.** O atacante consegue publicar um estado desejado, mas não consegue assinar artefactos com as chaves que os nós aceitam (princípio 5), não tem nenhuma mensagem para escrever em sinais (princípio 2) e não lê segredos, que vão cifrados para os nós (ADR 0024). Consegue, no máximo, fazer rollback para gerações já assinadas, limitado pela janela de manutenção e por `max_applies_per_day` do nó (ref/10 §4). **Aguenta**, com a condição de as chaves de assinatura **não** viverem no Deck (HSM ou assinatura offline no pipeline de release).

**D4 — Um integrador gere 40 clientes.** Multi-inquilino com isolamento por inquilino no inventário, nos artefactos privados e na telemetria. Os templates do integrador são partilhados; os sites são do cliente. **Aguenta.**

**D5 — Um cliente não quer nuvem.** Deck on-premises, ou nenhum Deck: `jk` e Git chegam (princípio 1). **Aguenta.**

**D6 — O Deck cresce até virar um SCADA.** O risco de *scope creep* é real. Contenção: o Deck gere **nós e configuração**, e mostra saúde e alarmes; sinópticos, HMI de operação e historian de planta continuam a ser de produtos dedicados (documento 12). Se isso mudar, é uma decisão de produto explícita, com uma ADR.

### 4.4 O Deck MVP na prática

O mínimo para correr o Deck MVP (§5, v1) numa instalação pequena: um servidor, algumas dezenas de nós, um repositório Git. É o que a trilha usa na parte 11, com o Deck no portátil. A referência completa do Deck (interface web, API de operadores, multi-inquilino, alta disponibilidade) é **produto**, e fica fora de `docs/ref/`. O contrato com os nós é só o protocolo `agent/1` da ref/10: tudo o que o Deck faz a um nó passa por lá.

#### 4.4.1 O binário e os comandos

| Comando | Faz |
|---|---|
| `joker-deck serve --config deck.toml` | arranca o Deck: endpoints `/agent/v1` da ref/10 §2.13, o armazém de artefactos, a leitura do Git e os rollouts |
| `joker-deck token create --node <nó> --ttl <d> [--config deck.toml]` | emite um token de enrolamento `jkt1_…` (ref/10 §2.5.1): de uso único, preso ao nome do nó, `ttl` ≤ 7 d |

A imagem de container equivalente é `registry.example.com/joker/deck`, com o mesmo binário como ponto de entrada:

```sh
docker run -d -p 8443:8443 -v /srv/deck:/srv/deck registry.example.com/joker/deck:1.0 serve --config /srv/deck/deck.toml
```

#### 4.4.2 `deck.toml`

```toml
# deck.toml — um Deck pequeno, num só servidor
[deck]
id     = "bancada-deck"
url    = "https://portatil.local:8443"
listen = "0.0.0.0:8443"
data   = "~/deck/data"

[database]
url = "sqlite:deck.db"                  # em produção: "postgres://deck@db.example.com/deck"

[git]
repo   = "~/joker-bancada"
branch = "main"
poll   = "30s"

[ca]
dir = "~/org-ca"                        # a mesma CA do `jk ca init`

[artifacts]
store = "builtin"                       # ou "oci://registry.example.com/joker/fleet"

[signing]
key = "~/.local/share/joker/keys/deck.key"
```

| Chave | Tipo | Obr. | Omissão | Significado |
|---|---|---|---|---|
| `deck.id` | nome (ref/00 §5) | sim | — | o `deck_id`: a `authority` do bundle de confiança, o `deck` de cada estado desejado e da chave `deck` (ref/10 §2.4.3, §2.6) |
| `deck.url` | URL `https://` | sim | — | o endereço que os nós usam; tem de estar nos `deck_urls` do bundle. O certificado TLS do servidor é emitido pela emissora de `[ca]` para o nome deste URL |
| `deck.listen` | `"<host>:<porta>"` | não | `"0.0.0.0:8443"` | onde o servidor escuta |
| `deck.data` | caminho | sim | — | estado local: a base SQLite, o armazém embutido, a cópia do repositório |
| `database.url` | `"sqlite:<caminho>"` \| `"postgres://…"` | não | `"sqlite:deck.db"` | inventário, rollouts, relatórios e eventos. SQLite (relativo a `data`) para uma instalação pequena; PostgreSQL em produção. A palavra-passe nunca vai no ficheiro: vem das variáveis do libpq (`PGPASSWORD`) ou de `~/.pgpass` |
| `git.repo` | URL ou caminho | sim | — | o repositório do projeto; as credenciais são as do Git do sistema (agente SSH, *credential helper*) |
| `git.branch` | string | não | `"main"` | o ramo que é o estado desejado da frota |
| `git.poll` | duração | não | `"30s"` | intervalo de leitura do ramo; ≥ `"5s"` |
| `ca.dir` | caminho | sim | — | a pasta do `jk ca init`: o Deck lê a emissora (`ca.crt`, `ca.key`), o `org` e o bundle de confiança (`trust.cose`), que entrega no enrolamento. **Nunca lê `root.key`**: a raiz assina bundles fora do Deck (§2, princípio 5) |
| `artifacts.store` | `"builtin"` \| URL `oci://` | não | `"builtin"` | onde vivem os artefactos assinados. `builtin`: uma API de distribuição OCI em `<url>/v2/`, guardada em `data`, e os nós descarregam de `/agent/v1/blobs` (fonte `kind = deck`); `oci://`: um registo externo, e os nós descarregam de lá (fonte `kind = oci`, com `registry-token`, ref/10 §2.13) |
| `signing.key` | caminho ou `pkcs11:` | sim | — | a chave Ed25519 **com o papel `deck`**, e só essa: assina estados desejados e comandos. O Deck recusa arrancar se esta chave tiver no bundle outro papel além de `deck` |

`~` no início de um caminho expande para `$HOME`; caminhos relativos resolvem a partir da pasta do `deck.toml`. Uma chave desconhecida recusa o arranque.

**O Deck não tem as chaves da organização.** Não tem a raiz, nem as chaves `config`, `apps` ou `secrets`. Por isso não fabrica gerações: lê do Git o que a geração de cada nó deve ser, calcula o descritor e o hash com os mesmos crates da `jk` (ref/10 §2.6.2), e procura no armazém o artefacto `joker.config.v1+cbor` assinado pelo papel `config` com esse descritor, e os artefactos que ele refere, assinados pelos seus papéis. Quem os assina e envia é a `jk`, na máquina de quem tem as chaves: `jk publish --manifest <m>… --registry <oci> --sign-key <papel>=<chave>…` (documento 07, §3) assina a geração de cada nó e as apps, e publica-as no registo de `artifacts.store` (com `builtin`, o registo OCI que o próprio Deck serve em `<deck.url>/v2/`). O Deck só procura artefactos assinados: sem eles, não emite nada para esse nó, e regista `J0915`.

#### 4.4.3 `fleet.toml`, no repositório

O Deck lê do ramo, na raiz do repositório, um `fleet.toml`: que nó corre que manifesto de entrada, em que coorte, e como se faz um rollout.

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

| Chave | Tipo | Obr. | Omissão | Significado |
|---|---|---|---|---|
| `version` | inteiro | sim | — | `1` |
| `nodes.<nó>.manifest` | ficheiro | sim | — | manifesto de entrada do nó, relativo à raiz do repositório; o lock é o do mesmo nome (ref/02 §2.1.1), e tem de ter sido capturado neste nó (`J0480`) |
| `nodes.<nó>.cohort` | nome | sim | — | a coorte do nó; aparece em `rollout.cohort` do estado desejado (ref/10 §2.6.4) |
| `rollout.wave` | lista de tabelas | sim | — | as ondas, por ordem; cada `cohorts` lista coortes, e cada coorte está em exatamente uma onda |
| `rollout.wait` | duração | não | `"30m"` | espera entre ondas, contada a partir do momento em que a onda passou (abaixo) |
| `rollout.confirm` | tabela | não | a da ref/10 §2.6.4 | a `confirm-policy` que vai no estado desejado: `mode`, `window`, `stable_for`, `require_uplink`, `alarm_severity` |
| `rollout.halt_if` | expressão ou lista de expressões | não | `[]` | condições sobre os KPIs da onda; basta uma verdadeira para parar o rollout |

**KPIs**, calculados pelo Deck a partir dos relatórios e dos eventos dos nós da onda (ref/10 §2.10, §2.11), desde a troca de cada nó:

| KPI | Significado |
|---|---|
| `overruns_rate` | overruns ÷ ticks, somados em todos os grupos de ciclo dos nós da onda (`report-body.groups`) |
| `trips_rate` | trips novos por nó e por hora (eventos `trip`) |
| `rollbacks` | nós da onda que terminaram este rollout em `rolled_back` |
| `baseline` | numa comparação, o valor do mesmo KPI nos mesmos nós na hora antes da troca (`"trips_rate > baseline * 1.5"`, D2) |

**De um commit a um rollout.**

1. A cada `poll`, o Deck lê o ramo. Um commit novo que muda a geração de algum nó (ou os seus segredos) abre um **rollout**, e o rollout anterior ainda em curso passa a `superseded`: deixa de emitir, e os nós que já o receberam ficam onde estão até o novo lá chegar.
2. Para cada nó, o Deck valida a geração como o `jk check` (V19–V22 da ref/10 §3.3) e procura os artefactos assinados (§4.4.2). Os segredos saem do `secrets.toml.enc` do commit: a cada nó vai o seu `secret-bundle`, só com o seu `wrap` (chave 22 do estado desejado); um `wrap` feito para um certificado que já não é o do nó é `J0922`, e esse nó fica em espera. Um nó que não passa não recebe nada; os outros seguem.
3. **Onda a onda.** O Deck emite o estado desejado para os nós da onda, com `rollout = { id, wave, cohort }` e o `confirm` do `fleet.toml`. A onda **passa** quando todos os nós online confirmaram, e pelo menos um confirmou, e depois de `wait` sem nenhuma condição de `halt_if` verdadeira. Um nó offline conta como **pendente**, nunca como falha (D1): recebe o estado desejado de hoje quando voltar.
4. **Paragem.** Uma condição de `halt_if` verdadeira, em qualquer momento da onda, põe o rollout em `halted`: o Deck não emite nada para as ondas seguintes, manda `reject` aos nós da onda que ainda estão a confirmar (ref/10 §2.7) e emite para os que já confirmaram um estado desejado com a geração anterior (`seq` + 1), que o nó reativa com o mesmo hash e o mesmo número (D2). Recomeça-se com um commit novo.
5. O Deck guarda **o último** estado desejado de cada nó, e nunca uma fila (ref/10 §3.11.5).

No MVP o Deck só **lê** o Git: não escreve commits, nem locks, nem aprovações. Um `fleet.toml` inválido não abre rollout nenhum: o Deck regista o erro e continua com o último válido.

---

## 5. Sequência

| Quando | O quê |
|---|---|
| v1 (Fase 3) | protocolo do agente especificado; **Deck MVP**: inventário, estado desejado a partir de Git, rollouts com ondas e paragem por KPI, saúde da frota; Hub = registo OCI + verificações de publicação |
| v1.x | Studio (local e no Deck); multi-inquilino; aprovação de intenções no Deck; sessões remotas |
| v1.5 (LTS; documento 21, §7; ref/10 §6) | observabilidade completa, auditoria agregada, MCUs autónomos geridos em escala |

O Deck é **outro produto**, com outra equipa quando crescer, mas partilha os crates de validação com a `jk`. É isso que impede que o Deck e o nó tenham ideias diferentes sobre o que é uma geração válida.

## 6. Nota sobre o modelo de negócio

Os princípios 1 e 3 (local-first e contratos abertos) são compatíveis com vários modelos. O mais natural é **runtime, Joker Micro, protocolo e formatos abertos**, e **Deck e Hub privado como produto** (SaaS ou licença on-premises). É uma decisão do dono do projeto, ligada à escolha da licença (documento 18, §6), e convém ser tomada antes do primeiro commit público.
