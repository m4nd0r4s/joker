# ref/10 — Protocolo do agente (nó ↔ Joker Deck), `agent/1`

Referência normativa do protocolo entre um nó (`jokerd` em Linux, ou um Joker Micro autónomo) e um plano de controlo da frota (o Joker Deck, ou qualquer outro que cumpra este documento). Aplica a ref/00 sem a repetir. Os esquemas máquina-legíveis estão em `spec/cddl/agent.cddl`.

Cinco regras que atravessam o documento inteiro (documento 20, §2):

1. **O nó funciona sem o Deck, para sempre.** Nada do que está aqui é necessário para o nó controlar. Um Deck em baixo, um certificado expirado ou um nó revogado nunca param uma saída.
2. **O protocolo não tem nenhuma mensagem de escrita em sinais.** O Deck muda configuração (gerações) e abre sessões; as escritas remotas só existem dentro de uma sessão, através da API local do nó, com o envelope, o árbitro e o TTL de sempre.
3. **O nó decide.** Cada documento e cada comando é verificado no nó contra chaves que o Deck não consegue mudar.
4. **Estado desejado completo e idempotente.** O Deck diz "onde o nó deve estar", nunca "o que fazer a seguir". Receber o mesmo documento duas vezes não faz nada.
5. **O Git é a fonte da verdade.** O protocolo transporta gerações identificadas por hash e por commit; não transporta edições.

---

## 1. Âmbito e fontes

### 1.1 O que cobre

| Superfície | Secção |
|---|---|
| identidade do nó, certificados, bundle de confiança, pisos de segurança | 2.4, 3.1, 3.2 |
| enrolamento (`jk node enroll`), renovação, revogação, nós clonados | 2.5, 3.1, 3.2 |
| estado desejado (Deck → nó), validação, aplicação por gerações, MCUs atrás do nó | 2.6, 3.3–3.7 |
| comandos (Deck → nó): confirmar, rejeitar, pedir relatório completo, renovar, carregar registos, sessões | 2.7 |
| estado reportado (nó → Deck) | 2.10, 3.8 |
| eventos (nó → Deck) | 2.11, 3.9 |
| sessão remota (túnel para a `jk` e o Studio) | 2.12, 3.10 |
| transportes: HTTPS com long-poll, MQTT, CoAP (perfil micro) | 2.13–2.15, 3.11 |
| perfil reduzido para MCUs autónomos | 2.15, 3.13 |
| chaves `[agent]` do `node.toml` (acolhidas na ref/02) | 2.17 |
| versões e compatibilidade | 3.14, 6 |

Não cobre: a telemetria (Sparkplug B, Zenoh, OTLP; documento 20, §3.3), a API do Hub, o formato interno dos artefactos (ref/02, ref/08), o Joker Link (ref/07) nem o desenho interno do Deck.

### 1.2 Fontes

Documento 20 (§2, §3.1, §4), documento 21 (§1, §7, §8), ADR 0015, 0021 e 0022, documento 01 (§12, §13, §15–§18), documento 12 (§7, §11), documento 19 (§3, §4.1, §6), documento 09 (ataques 8, 9 e 11), documento 08 (rodada 5), documento 07 (§3), documento 15 (§2.5) e a ref/00.

### 1.3 Ligações a outras referências

| Referência | O que partilha com esta |
|---|---|
| ref/00 | versões, tempo, identificadores, enums partilhados, codificações, erros, segurança |
| ref/01 | `[security]` do manifesto: `trust` (só restringe, §2.4.3) e os principais `deck:` em `[security.roles]` (§3.10); o hash do manifesto efetivo (ref/01 §8.3), que entra no descritor da geração (§2.6.2) |
| ref/02 | `node.toml` (as âncoras de confiança, `[policy] allow_dev` e `apply_from`, e a secção `[agent]` do §2.17), o digest canónico do lock (§2.6.2), os segredos por destinatário e o `secret-bundle` (chave 22 do estado desejado), a chave KEM do nó |
| ref/07 | mensagem `OTA` e `HELLO` do Joker Link, usadas na fase `mcu` (§3.6) |
| ref/08 | formato e assinatura do blob `joker.generation.v1` e do firmware do Joker Micro |
| ref/09 | endpoints locais `/v1/agent/*` e `/v1/node/*` (§2.18), o trinco de transição de gerações partilhado com a API (§3.4.2), principais pré-autenticados e auditoria das sessões remotas (§3.10) |

### 1.4 Vocabulário

| Termo | Significado |
|---|---|
| **autoridade** | o Deck (`deck_id`) com que o nó está enrolado. Um nó tem no máximo uma autoridade de cada vez |
| **DSD** | documento de estado desejado: `desired-state` assinado em COSE_Sign1 |
| **bundle** | bundle de confiança: as chaves públicas que o nó aceita, por papel, assinado pela raiz da organização |
| **piso** | *floor statement*: versão mínima e digests revogados de um tipo de artefacto |
| **fase** | uma das três partes de uma aplicação: `runtime`, `mcu`, `generation` (§3.4) |
| **núcleo mínimo** | as mensagens `hello`, `poll`/`desired`, `report`, `events` e `renew` na forma `agent/1.0`; nunca mudam na série 1 (§3.14) |

---

## 2. A referência

### 2.1 Codificação e forma dupla

| Onde | Codificação |
|---|---|
| DSD, comandos, bundles, pisos, provas | CBOR determinístico (RFC 8949, §4.2.1) dentro de COSE_Sign1 (RFC 9052). **Sempre**, também em HTTPS: um documento assinado é opaco para o transporte |
| mensagens em HTTPS | JSON (RFC 8259), UTF-8, com chaves de texto; CBOR com chaves inteiras se o nó anunciar `cbor_https` e o Deck o aceitar (`Accept: application/cbor`) |
| mensagens em MQTT e CoAP | CBOR com chaves inteiras; em MQTT, cada mensagem vai em COSE_Sign1 (§2.14, §3.11.2) |

**Forma dupla do CDDL.** Cada membro escreve-se `("nome" / n)`. Em CBOR usa-se só `n` e as formas binárias; em JSON só `"nome"` e as formas de texto. Misturar as duas numa mensagem é `bad_request` (`J0978`).

| Tipo CDDL | CBOR | JSON |
|---|---|---|
| `time` | `int`, ns UTC desde 1970 | RFC 3339 com `Z` (ref/00, §3) |
| `duration` | `uint`, ns | literal de duração (`"10m"`) |
| `digest` | `bstr` de 32 B | `"sha256:<64 hex>"` |
| `ulid` | `bstr` de 16 B | 26 caracteres Crockford base32 |
| `kid` | `bstr` de 16 B: os 16 primeiros bytes do SHA-256 da chave pública crua | 32 hex |
| `enum` | valor numérico (§2.2) | nome `snake_case` |
| `bytes`, `cose` | `bstr` | base64 (RFC 4648, §4, com `=`) |
| `seq` | `uint` < 2⁵³ | número |

**Chaves inteiras.** 1–99: v1. 100–119: reservadas para a v2 (sites, §6). Negativas em CBOR, e de texto começadas por `x_` em JSON: extensões privadas, sempre ignoradas. Uma chave removida nunca se reutiliza (ref/00, §1).

**Leitura tolerante.** Um leitor ignora membros desconhecidos e mapeia um valor de enum desconhecido para a categoria indicada em §2.2 (ou `unknown`). A exceção é o DSD: uma chave listada em `crit` (chave 20) que o nó não conheça faz recusar o documento (`J0946`). É o mecanismo que impede um nó antigo de aplicar metade de um estado desejado novo.

**COSE.** Cabeçalho protegido: `alg` (1) = −8 (EdDSA) ou −7 (ES256); `kid` (4); `content type` (3) com um dos valores:

| Documento | `content type` | Assinado por (papel) |
|---|---|---|
| `desired-state` | `application/joker-desired+cbor` | `deck` |
| `command` | `application/joker-command+cbor` | `deck` |
| `trust-bundle` | `application/joker-trust+cbor` | `root` |
| `floor-statement` | `application/joker-floor+cbor` | o papel do tipo de artefacto |
| `renew-proof` | `application/joker-renew+cbor` | chave de identidade do nó (antiga) |
| mensagens em MQTT (nó → Deck) | `application/joker-agent+cbor` | chave de identidade do nó |
| mensagens em MQTT (Deck → nó, sem DSD nem comando) | `application/joker-agent+cbor` | `deck` |

Sem `external_aad`. Não há contra-assinaturas nem assinaturas múltiplas na v1.

### 2.2 Enumerações

As partilhadas vêm da ref/00, §6: `clock_quality`, `alarm_state`, `severity`, `role`, `generation_state`, e os resultados e motivos de escrita da ref/00, §4.4. As próprias do protocolo:

| Enum | Valores | Desconhecido lê-se como |
|---|---|---|
| `agent-status` | `ok` 0, `retry` 1, `need_full` 2, `bad_request` 3, `unauthorized` 4, `forbidden` 5, `conflict` 6, `unsupported` 7, `revoked` 8, `too_large` 9, `rate_limited` 10, `quarantined` 11, `not_found` 12 | `retry` |
| `trust-role` | `root` 0 (âncora: assina bundles; as `trust.anchors` do `node.toml`, ref/02 §2.5.4), `deck` 1, `runtime` 2, `apps` 3, `config` 4, `profiles` 5, `boards` 6, `classes` 7, `firmware` 8, `models` 9 (reservado 1.1), `drivers` 10, `secrets` 11. Os nomes são os papéis da ref/02 §2.5.3 e da ref/00 §7.1 | ignorado (a chave não serve para nada) |
| `key_storage` | `tpm` 0, `secure_element` 1, `file` 2 | `file` |
| `node-kind` | `linux` 0, `micro` 1 | — (recusa) |
| `mode` | `dev` 0, `prod` 1 | `prod` |
| `capability` | `mqtt` 0, `long_poll` 1, `sessions` 2, `cbor_https` 3, `import` 4, `mcus` 5, `models` 6 (1.1), `attestation` 7 | ignorada |
| `source-kind` | `oci` 0, `deck` 1, `mirror` 2 | fonte ignorada |
| `source-auth` | `mtls` 0, `token` 1, `none` 2 | fonte ignorada |
| `confirm-mode` | `auto` 0, `deck` 1, `manual` 2 | recusa (`J0979`) |
| `phase` | `runtime` 0, `mcu` 1, `generation` 2 | recusa |
| `apply-state` | `accepted` 0, `validating` 1, `fetching` 2, `staging` 3, `waiting` 4, `applying` 5, `confirming` 6, `confirmed` 7, `rolled_back` 8, `failed` 9, `refused` 10, `superseded` 11, `noop` 12 | — |
| `wait-reason` | `not_before` 0, `window` 1, `jitter` 2, `clock` 3, `paused` 4, `local_hold` 5, `confirmation` 6, `rate_limit` 7, `mcu_link` 8 | — |
| `command-kind` | `confirm` 0, `reject` 1, `report_full` 2, `renew` 3, `upload` 4, `session_open` 5 (1.3), `session_close` 6 (1.3) | `unsupported` |
| `upload-kind` | `decisions` 0, `logs` 1, `trace` 2, `events` 3 | `unsupported` |
| `health` | `ok` 0, `degraded` 1, `failed` 2, `unknown` 3 | `unknown` |
| `group-state` | `ok` 0, `degraded` 1, `overrun` 2, `tripped` 3, `stopped` 4 | `degraded` |
| `link-state` | `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5 (o `link_state` da ref/00 §6) | `degraded` |
| `instance-health` | `ok` 0, `degraded` 1, `failed` 2 (documento 14, A8) | `degraded` |
| `app-state` | `starting` 0, `running` 1, `crashloop` 2, `stopped` 3, `failed` 4, `shadow` 5, `updating` 6 (o `app_state` da ref/00 §6) | `failed` |
| `gen-origin` | `agent` 0, `local` 1, `import` 2, `session` 3, `shadow` 4, `rollback` 5, `boot` 6 (a origem de cada **ativação**, a mesma da API, ref/09 §5.13) | `local` |
| `transport` | `https` 0, `mqtt` 1, `coap` 2 | — |
| `slot` | `a` 0, `b` 1 | — |
| `event-type` | `alarm` 0, `decision` 1, `apply` 2, `trip` 3, `security` 4, `session` 5, `gap` 6, `clock` 7, `command_result` 8, `cert` 9, `agent` 10 | guardado tal qual |
| `trip-action` | `tripped` 0, `reset` 1 | — |
| `security-kind` | `desired_refused` 0, `command_refused` 1, `trust_refused` 2, `floor_refused` 3, `artifact_refused` 4, `clone_suspected` 5, `session_refused` 6, `tls_pin` 7 | guardado tal qual |
| `session-action` | `requested` 0, `opened` 1, `closed` 2, `refused` 3, `expired` 4 | — |
| `cert-action` | `renewed` 0, `expiring` 1, `expired` 2, `revoked` 3 | — |
| `agent-action` | `paused` 0, `resumed` 1, `hold` 2, `imported` 3, `offline` 4, `online` 5 | — |
| `tunnel-kind` | `open` 0, `data` 1, `close` 2, `reset` 3, `credit` 4, `ping` 5 | fecha o túnel |

### 2.3 Respostas e erros

Uma resposta de erro é um `status-reply`:

| # | Nome | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `status` | `agent-status` | sim | categoria (decide o que o cliente faz) |
| 2 | `code` | J-code | não | a causa, do registo (§2.20, ref/00, §8) |
| 3 | `detail` | tstr ≤ 1 KiB | não | texto para pessoas, em inglês |
| 4 | `retry_after` | duração | não | espera mínima antes de repetir |

Em HTTPS, o `status` acompanha o código HTTP:

| `agent-status` | HTTP | O cliente |
|---|---|---|
| `ok` | 200 | — |
| `retry` | 503 | repete com backoff (§3.11.4) |
| `need_full` | 409 | envia um relatório completo |
| `bad_request` | 400 | não repete a mesma mensagem; regista `J0978` localmente |
| `unauthorized` | 401 | renova o certificado (§3.2) |
| `forbidden` | 403 | não repete; alarme local |
| `conflict` | 409 | relê o estado (hello) |
| `unsupported` | 501 | desiste da funcionalidade |
| `revoked` | 410 | pára o agente; alarme `joker.agent.revoked` |
| `too_large` | 413 | parte ou trunca (§3.8) |
| `rate_limited` | 429 | espera `retry_after` |
| `quarantined` | 403 | continua a reportar, não espera estado desejado (§3.2.3) |
| `not_found` | 404 | — |

### 2.4 Identidade e confiança

#### 2.4.1 Chaves

| Chave | Algoritmo | Onde vive | Para quê |
|---|---|---|---|
| **identidade do nó** | ECDSA P-256 (ES256) | TPM 2.0 ou elemento seguro quando existe; senão ficheiro `0600` em `/var/lib/joker/identity/` (aviso `J0933`); num MCU, elemento seguro ou flash protegida | mTLS com o Deck e o broker; assinatura das mensagens em MQTT; prova de renovação |
| **raiz da organização** (`root`) | Ed25519 | fora de linha (HSM, cartão, cerimónia) | assina bundles de confiança (as âncoras, ref/02 §2.5.4) |
| **Deck** (`deck`) | Ed25519 | no Deck, de preferência em KMS/HSM | assina DSD, comandos e mensagens Deck → nó em MQTT |
| **artefactos** (`runtime`, `apps`, `config`, `profiles`, `boards`, `classes`, `drivers`, `firmware`, `secrets`, `models`) | Ed25519 | **fora do Deck**: pipeline de release, KMS, assinatura fora de linha | assinam artefactos OCI (cosign/notation), pisos e, `secrets`, as entradas de segredos (ref/02 §2.4) |

D7 e ADR 0025: a identidade dos nós é **P-256**, porque é o que os TPM, os elementos seguros (ATECC608 e afins) e as pilhas TLS de MCU (mbedTLS) suportam; as chaves da organização são **Ed25519**. No COSE, `EdDSA` (−8) é o algoritmo das chaves da organização e `ES256` (−7) o das assinaturas feitas pelo próprio nó (mensagens MQTT, prova de renovação). Um `trust-key` com `alg` ≠ −8 é recusado (`J0939`).

#### 2.4.2 Certificado do nó

| Campo | Valor |
|---|---|
| versão | X.509 v3 |
| sujeito | `CN=<nó>, O=<org>` |
| SAN | `URI:urn:joker:<org>:node:<nó>` (Linux) ou `URI:urn:joker:<org>:micro:<nó>` (MCU autónomo); `DNS:<nó>`; e a chave KEM X25519 de segredos em `URI:urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3, §3.4.2) |
| EKU | `clientAuth` |
| chave | P-256 |
| validade | omissão 365 d; entre 30 d e 5 anos (política do Deck) |
| emissor | CA emissora da organização (pode viver no Deck; a sua compromissão não dá acesso aos nós, §4, S1) |

#### 2.4.3 Bundle de confiança (`trust-bundle`)

Assinado por uma chave `root`. É o que o nó usa para verificar tudo o resto.

| # | Nome | Tipo | Obr. | Omissão | Validação | Erro |
|---|---|---|---|---|---|---|
| 1 | `v` | `1` | sim | — | = 1 | `J0939` |
| 2 | `org` | nome | sim | — | = org do nó | `J0939` |
| 3 | `version` | seq | sim | — | > versão instalada | `J0939` |
| 4 | `issued` | tempo | sim | — | — | — |
| 5 | `authority` | nome | não | sem Deck | uma chave `deck` com esse `deck` | `J0939` |
| 6 | `keys` | `[+ trust-key]` | sim | — | ≤ 32; pelo menos uma `root` | `J0939` |
| 7 | `deck_urls` | `[+ tstr]` | não | — | `https://`, ≤ 4 | `J0939` |
| 8 | `deck_tls` | `[+ digest]` | não | WebPKI | SHA-256 do SPKI aceite para o Deck e o broker | — |
| 9 | `mqtt` | `[+ tstr]` | não | — | `mqtts://`, ≤ 4 | `J0939` |
| 10 | `coap` | `[+ tstr]` | não | — | `coaps://`, ≤ 4 | `J0939` |
| 11 | `scope` | `[+ tstr]` | não | `["*"]` | nomes de nós ou `site:<s>`; o nó tem de estar incluído | `J0939` |
| 12 | `min_mode` | `mode` | não | `dev` | o nó recusa gerações com `mode` abaixo deste; combina-se com `[policy] allow_dev` do `node.toml` (ref/02 §3.5.1): uma geração `dev` precisa das duas coisas | `J0956`, `J0906` |

`trust-key`:

| # | Nome | Tipo | Obr. | Validação |
|---|---|---|---|---|
| 1 | `kid` | kid | sim | = SHA-256(`key`)[0..16]; único no bundle |
| 2 | `alg` | int | sim | −8 (Ed25519, D7) |
| 3 | `key` | bytes | sim | 32 B |
| 4 | `roles` | `[+ trust-role]` | sim | `root` não se combina com outros papéis |
| 5 | `not_after` | tempo | não | verificada só com relógio `synced` |
| 6 | `deck` | nome | se `deck` ∈ `roles` | o `deck_id` que a chave representa |

**Regra de atualização.** Um bundle novo só é aceite se for assinado por uma chave `root` **do bundle instalado** e tiver `version` maior. Uma rotação da raiz é um bundle assinado pela raiz antiga que já inclui a nova. Um nó que esteve fora durante várias rotações recebe a **cadeia** de bundles e verifica-os um a um (`renew-reply.trust`, `hello-reply.trust`, `GET /trust`).

**Manifesto.** O `[security] trust` do manifesto (documento 03, §13) **só restringe**: cada chave lá referida tem de estar no bundle com o papel correspondente. Uma chave que não esteja é `J0292`, a validação do manifesto (ref/01), no `jk check` e no V20 do staging. Assim, quem assina configuração não consegue alargar quem assina runtime ou apps.

#### 2.4.4 Pisos de segurança (`floor-statement`)

Assinado por uma chave com o papel do tipo de artefacto (o piso da runtime pela chave `runtime`, e assim por diante).

| # | Nome | Tipo | Obr. | Validação |
|---|---|---|---|---|
| 1 | `v` | `1` | sim | = 1 |
| 2 | `role` | `trust-role` | sim | ≠ `root`, ≠ `deck` |
| 3 | `seq` | seq | sim | > último piso aceite para (`role`, `scope`) |
| 4 | `issued` | tempo | sim | — |
| 5 | `min_version` | semver | não | versão mínima admissível |
| 6 | `revoked` | `[* digest]` | não | ≤ 256 digests proibidos |
| 7 | `scope` | tstr | não, `"*"` | placa, arquitetura (`arch:aarch64`) ou `*` |
| 8 | `min_counter` | uint | não | contador de segurança mínimo (firmware de MCU) |

O nó guarda o piso de maior `seq` por (`role`, `scope`), com fsync. Um piso inválido faz recusar o DSD inteiro (`J0939`).

### 2.5 Enrolamento e renovação

#### 2.5.1 Token de enrolamento

```
jkt1_<org>_<kid-da-raiz, 32 hex>_<segredo, 32 caracteres base32>
```

O `_` separa sem ambiguidade, porque não existe em nomes (ref/00, §5). O token é de uso único, expira (omissão 24 h, máximo 7 d) e pode estar preso a um nome de nó ou a um site. O `kid` da raiz permite ao nó verificar o bundle que recebe sem confiar no canal: é o único *trust on first use* do protocolo, e é explícito.

#### 2.5.2 `enroll-request` (nó → Deck, `POST /enroll`, sem mTLS)

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `token` | tstr | sim | válido, por usar, não expirado | `J0930` |
| 2 | `node` | nome | sim | gramática da ref/00, §5; livre na org ou igual ao do token | `J0932` |
| 3 | `csr` | bytes | sim | PKCS#10 DER, P-256, assinado pela própria chave; `CN` = `node`; com a chave KEM X25519 no SAN URI `urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3) | `J0932` |
| 4 | `hw` | digest | sim | impressão do hardware (§3.2.3) | — |
| 5 | `key_storage` | enum | sim | o Deck pode exigir `tpm`/`secure_element` | `J0933` |
| 6 | `kind` | `node-kind` | sim | — | — |
| 7 | `board` | tstr | não | — | — |
| 8 | `runtime` | semver | não | — | — |
| 9 | `attestation` | `attestation` | não | reservado; o Deck pode exigi-lo a partir da 1.3 | `J0933` |

`attestation` (TPM 2.0): `kind` (1, `tpm2` 0), `ek_cert` (2, certificado da *endorsement key*, DER), `ak` (3, chave pública da *attestation key*), `quote` (4, `TPM2_Quote` sobre os PCR 0–7 e o `nonce`), `nonce` (5, obtido em `GET /enroll/nonce`, válido 5 min). A verificação é do Deck; o formato fica fixado já para não mudar na 1.3.

#### 2.5.3 `enroll-reply`

| # | Nome | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `cert` | `[+ bytes]` | sim | cadeia DER, folha primeiro, até à CA emissora |
| 2 | `trust` | cose | sim | bundle assinado; o nó verifica que a raiz tem o `kid` do token (`J0931`) |
| 3 | `deck` | nome | não | `deck_id` da autoridade (= `trust.authority`) |
| 4 | `settings` | `agent-settings` | não | valores iniciais (§2.9) |
| 5 | `anchors` | `[+ trust-key]` | sim | as chaves `root` da organização: o nó escreve-as em `trust.anchors` do `node.toml` (ref/02 §3.2.3, passo 4); uma delas tem o `kid` do token, e o bundle da chave 2 tem de estar assinado por uma delas |

#### 2.5.4 Renovação (`POST /renew`, mTLS)

`renew-request`: `csr` (1, chave de identidade **nova** em cada renovação, com a **mesma** chave KEM no SAN, que só muda num novo enrolamento, ref/02 §3.2.3), `proof` (2, COSE_Sign1 da chave antiga sobre `renew-proof`), `hw` (3). `renew-proof`: `node` (1), `csr` (2, SHA-256 do CSR), `ts` (3), `nonce` (4, ULID). `renew-reply`: `cert` (1) e, se o nó estiver atrasado, a cadeia de bundles (2).

O Deck aceita o mTLS com o certificado antigo **mesmo expirado**, até `renew_grace` depois do fim (política do Deck, omissão 730 d), desde que não esteja revogado e a prova confira (`J0936` se não).

### 2.6 Estado desejado (`desired-state`)

| # | Nome | Tipo | Obr. | Omissão | Validação | Erro |
|---|---|---|---|---|---|---|
| 1 | `v` | `1` | sim | — | = 1 | `J0946` |
| 2 | `node` | nome | sim | — | = nome do nó | `J0944` |
| 3 | `org` | nome | sim | — | = org do nó | `J0944` |
| 4 | `deck` | nome | sim | — | = `authority` do bundle e = `deck` da chave | `J0941` |
| 5 | `seq` | seq | sim | — | > último aceite (§3.3) | `J0942`, `J0943` |
| 6 | `issued` | tempo | sim | — | ≤ agora + 5 min (só com relógio `synced`) | `J0945` |
| 7 | `expires` | tempo | não | sem expiração | > agora (só com relógio `synced`) | `J0945` |
| 8 | `runtime` | `runtime-target` | não | manter a atual | §2.6.1 | `J0947`, `J0948` |
| 9 | `generation` | `generation-target` | não | manter a atual | §2.6.2 | `J0954`, `J0955` |
| 10 | `artifacts` | `[* artifact]` | sim | — | ≤ 256; todos os digests referidos, e só esses (§3.3, V12 e V18) | `J0949` |
| 11 | `sources` | `[+ source]` | não | `[{url = <deck>/agent/v1/blobs, kind = deck}]` | ≤ 8 | `J0946` |
| 12 | `confirm` | `confirm-policy` | não | §2.6.4 | §2.6.4 | `J0979` |
| 13 | `rollout` | `rollout-info` | não | — | informativo; volta nos eventos | — |
| 14 | `schedule` | `schedule` | não | aplicar já | §2.6.5 | `J0979` |
| 15 | `mcus` | `[* mcu-target]` | não | manter os atuais | ≤ 32; `device` existe na geração alvo com o driver `joker-link` | `J0949` |
| 16 | `trust` | `[+ cose]` | não | — | bundles novos, por ordem de versão | `J0939` |
| 17 | `floors` | `[+ cose]` | não | — | pisos novos | `J0939` |
| 18 | `settings` | `agent-settings` | não | — | limitadas às margens do nó (§2.9) | — |
| 19 | `override_local` | bool | não | `false` | se `true`, substitui uma geração alterada localmente (§3.7) | — |
| 20 | `crit` | `[+ uint]` | não | — | chaves que o nó tem de conhecer | `J0946` |
| 21 | `note` | tstr ≤ 1 KiB | não | — | texto para pessoas (aparece no `jk agent desired`) | — |
| 22 | `secrets` | bytes | não | — | o `secret-bundle` deste nó (tipo de media `joker.secrets.v1`, `spec/cddl/secrets.cddl`, ref/02 §2.4): as entradas cifradas para a chave KEM do nó, cada uma assinada pelo papel `secrets`. O Deck entrega-as sem as conseguir ler (D6, ADR 0024). Verificadas entrada a entrada no V10b (§3.3) | `J0922`–`J0926` (por entrada) |
| 23 | — | — | — | — | reservado: modelos (1.1) | — |
| 100–119 | — | — | — | — | reservado: v2 (sites) | — |

Um `runtime`, `generation` ou `mcus` ausente quer dizer "sem opinião": o nó mantém o que tem. Presente, é o alvo completo dessa parte.

#### 2.6.1 `runtime-target`

| # | Nome | Tipo | Obr. | Omissão | Validação | Erro |
|---|---|---|---|---|---|---|
| 1 | `version` | semver | sim | — | = `org.joker.version` do artefacto | `J0948` |
| 2 | `artifact` | digest | sim | — | em `artifacts`, tipo `joker.runtime.v1` | `J0949` |
| 3 | `allow_downgrade` | bool | não | `false` | necessário se `version` < atual | `J0947` |
| 4 | `health_window` | duração | não | `5m` | 1 min–1 h | `J0979` |

O manifesto OCI da runtime leva as anotações `org.joker.version`, `org.joker.arch`, `org.joker.boards` (lista, ou `*`), `org.joker.agent` (minor do protocolo), `org.joker.state_min` (versão mínima do formato de `/var/lib/joker` que lê) e `org.joker.worlds` (mundos WIT aceites). O nó lê-as antes de descarregar o binário.

#### 2.6.2 `generation-target`

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `hash` | digest | sim | = hash calculado no staging (§3.4.3) | `J0955` |
| 2 | `config` | digest | sim | em `artifacts`, tipo `joker.config.v1` | `J0949` |
| 3 | `label` | tstr ≤ 128 B | não | — | — |
| 4 | `source` | `git-ref` | não | `repo` (1), `commit` (2), `path` (3): de onde veio, para auditoria | — |

**Descritor e hash da geração.** Esta secção é a dona da definição (documento 22, §5). Uma geração de um nó é descrita por um **descritor** em CBOR determinístico (`generation-descriptor` no CDDL):

| # | Nome | Tipo | Obr. | Conteúdo |
|---|---|---|---|---|
| 1 | `v` | `1` | sim | versão do descritor |
| 2 | `kind` | `node-kind` | sim | `linux` ou `micro` |
| 3 | `manifest` | mapa CBOR | sim | o manifesto efetivo **com as omissões materializadas e sem as chaves `x_*`** (ref/01 §8.3), em CBOR determinístico com as chaves TOML como `tstr` |
| 4 | `lock` | digest | sim | o **digest canónico** do `joker.lock` (ref/02 §2.1: SHA-256 da serialização canónica, e não dos bytes do ficheiro) |
| 5 | `contracts` | `[* digest]` | sim | os hashes dos contratos das classes usadas (documento 17, §6), por ordem de bytes |
| 6 | `artifacts` | `[* gen-artifact]` | sim | cada ficheiro que a geração refere e que muda o comportamento (apps, drivers, perfis, placas, modelos de simulação, chaves de confiança, EDS, …; ref/01 §8.3, ponto 4): `name` (1, como o manifesto o refere: `apps.peak_shaver`, `profiles/local/xy-md02@1.profile.toml`), `media_type` (2, forma curta da ref/00 §7.1), `digest` (3); por ordem de `name` |
| 7 | `mode` | `mode` | sim | o `mode` do manifesto (para o V19 sem abrir o manifesto) |

```
gen_hash = SHA-256( det-CBOR( generation-descriptor ) )
```

- A **proveniência** (ficheiro e linha de cada chave, ref/09 §5.13) não entra: não muda o comportamento.
- Reordenar chaves, mudar comentários ou partir um ficheiro em overlays não muda o manifesto efetivo, e por isso não muda o hash; mudar o conteúdo de um perfil local ou de uma chave de confiança muda-o (ponto 6).
- A **API local** (ref/09 §5.13) e o **Joker Micro** (o `source_hash` do blob, ref/08 §2.13.2, com `kind = micro`) usam este mesmo cálculo. O `jk`, o `jokerd`, o Deck e o MCU mostram todos o mesmo hash.

**`config` e `hash`.** O artefacto `joker.config.v1+cbor` (ref/00 §7.1) é um COSE_Sign1 assinado pelo papel `config` cuja carga útil é o descritor. `generation-target.config` é o **digest desse artefacto** (com a assinatura, para o descarregar e verificar); `generation-target.hash` é o **hash do descritor** (sem a assinatura), que identifica a geração em todo o lado. No staging (V21), o nó reconstrói o descritor a partir do manifesto, do lock e dos artefactos descarregados, e compara o seu hash com `hash` e com a carga útil do artefacto `config` (`J0955`). O manifesto efetivo vai dentro do descritor; o lock viaja como artefacto do DSD (`joker.lock.v1+toml`), e o nó verifica o seu digest canónico contra a chave 4; os outros ficheiros, pelos digests da chave 6.

#### 2.6.3 `artifact` e `source`

`artifact`:

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `digest` | digest | sim | único na lista | `J0949` |
| 2 | `media_type` | tstr | sim | da tabela do §2.16 | `J0957` |
| 3 | `size` | uint | sim | bytes; o nó recusa a descarga se exceder | `J0950` |
| 4 | `ref` | tstr | não | referência OCI legível, para pessoas | — |
| 5 | `signatures` | `[+ digest]` | não | manifestos de assinatura, para registos sem a API *referrers* | — |

`source`:

| # | Nome | Tipo | Obr. | Omissão | Significado |
|---|---|---|---|---|---|
| 1 | `url` | tstr | sim | — | base de um registo OCI (`https://registry.example.com`) ou do proxy do Deck |
| 2 | `kind` | `source-kind` | não | `oci` | `deck`: `GET /agent/v1/blobs/{digest}`; `mirror`: registo local do site |
| 3 | `auth` | `source-auth` | não | `mtls` para `deck`, `token` para `oci` | `token`: o nó pede um `registry-token` ao Deck (§2.13) |

Os espelhos da configuração do nó (`[agent] mirrors`) são tentados **antes** das fontes do DSD. A origem não importa: o conteúdo é verificado por digest e por assinatura.

#### 2.6.4 `confirm-policy`

| # | Nome | Tipo | Omissão | Validação | Significado |
|---|---|---|---|---|---|
| 1 | `mode` | `confirm-mode` | `auto` | — | `auto`: o nó confirma-se a si próprio pela saúde (§3.5); `deck`: espera um comando `confirm`; `manual`: espera um `jk confirm` local (ou por sessão) |
| 2 | `window` | duração | `10m` | 1 min–24 h; ≥ `stable_for` + 1 min | janela de confirmação da ADR 0015 |
| 3 | `require_uplink` | bool | `true` | — | a confirmação exige uma troca autenticada com o Deck depois da troca de geração |
| 4 | `stable_for` | duração | `60s` | 10 s–1 h | tempo seguido de saúde exigido em `auto` |
| 5 | `alarm_severity` | `severity` | `critical` | — | um alarme novo desta gravidade ou maior falha a saúde |

Uma política inválida é `J0979`. O `confirm` do DSD aplica-se às fases `runtime` e `generation`; cada `mcu-target` pode ter o seu.

`rollout-info` é só informação para o Deck, devolvida nos eventos `apply` e em `desired.rollout`: `id` (1, ULID do rollout, obrigatório), `wave` (2, número da onda), `cohort` (3, nome da coorte, ≤ 64 B). O nó não decide nada com ela.

#### 2.6.5 `schedule` e `window`

| # | Nome | Tipo | Omissão | Validação | Significado |
|---|---|---|---|---|---|
| 1 | `not_before` | tempo | — | — | não trocar antes deste instante (o download pode começar já) |
| 2 | `windows` | `[+ window]` | sempre | ≤ 16 | só trocar dentro de uma destas janelas, intersetadas com as do nó (§3.4.4) |
| 3 | `jitter` | duração | `0s` | ≤ 24 h | atraso aleatório uniforme, sorteado uma vez por DSD e guardado |
| 4 | `fetch_deadline` | duração | `72h` | 1 h–30 d | prazo para descarregar tudo, contado da aceitação |

`window`: `days` (1, `[+ 1..7]`, ISO 8601, 1 = segunda), `start` (2, `"HH:MM"`), `length` (3, duração ≤ 24 h), `tz` (4, IANA, omissão `"UTC"`). Uma janela que atravessa a meia-noite pertence ao dia em que começa. Nas mudanças de hora, conta a hora local de parede; uma hora que não existe começa na seguinte.

#### 2.6.6 `mcu-target` (MCUs atrás do nó)

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `device` | caminho | sim | dispositivo com driver `joker-link` na geração alvo | `J0949` |
| 2 | `firmware` | `mcu-firmware` | não | `version` (1), `artifact` (2, tipo `joker.micro-firmware.v1`), `allow_downgrade` (3, omissão `false`) | `J0963` |
| 3 | `generation` | `mcu-generation` | não | `hash` (1), `artifact` (2, tipo `joker.generation.v1`) | `J0964` |
| 4 | `confirm` | `confirm-policy` | não | omissão: `auto`, `window = "5m"`, `stable_for = "30s"`, `require_uplink = false` | `J0979` |
| 5 | `order` | uint | não, 0 | ordem de atualização; empates por `device` | — |

#### 2.6.7 Exemplo (vista JSON, como a mostra `jk agent desired --json`)

```json
{
  "v": 1,
  "node": "hb-01234",
  "org": "example",
  "deck": "deck-prod",
  "seq": 418,
  "issued": "2026-09-24T09:00:00Z",
  "expires": "2026-10-01T09:00:00Z",
  "generation": {
    "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5",
    "config": "sha256:9a0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8f",
    "label": "hp-v3 tarifa epex",
    "source": { "repo": "https://git.example.com/fleet/sites.git", "commit": "e3b1f07", "path": "sites/joker.hb-01234.toml" }
  },
  "artifacts": [
    { "digest": "sha256:9a0b1c2d3e4f5061728394a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8f", "media_type": "joker.config.v1+cbor", "size": 18244 },
    { "digest": "sha256:1f2e3d4c5b6a79880716253443526170f1e2d3c4b5a69788f1e2d3c4b5a69788", "media_type": "joker.app.v1+wasm", "size": 2811904, "ref": "registry.example.com/apps/hp-optimizer:3.2.0" }
  ],
  "sources": [ { "url": "https://registry.example.com", "kind": "oci", "auth": "token" } ],
  "confirm": { "mode": "auto", "window": "15m", "stable_for": "5m" },
  "rollout": { "id": "01J8ZK3M4N5P6Q7R8S9T0V1W2X", "wave": 2, "cohort": "cm4-din" },
  "schedule": { "windows": [ { "days": [1, 2, 3, 4, 5], "start": "02:00", "length": "3h", "tz": "Europe/Berlin" } ], "jitter": "45m" }
}
```

### 2.7 Comandos (`command`, Deck → nó)

Um comando é um pedido pontual, assinado pela chave `deck`, **idempotente e preso ao seu alvo**. Não há comandos de escrita em sinais, de reinício do nó, nem de mudança de confiança.

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `id` | ULID | sim | um `id` repetido devolve o resultado anterior (sem erro) | — |
| 2 | `node` | nome | sim | = nó | `J0944` |
| 3 | `deck` | nome | sim | = autoridade | `J0941` |
| 4 | `ts` | tempo | sim | com relógio `synced`: \|ts − agora\| ≤ 30 s (ref/00, §9) | `J0968` |
| 5 | `kind` | `command-kind` | sim | — | `unsupported` |
| 6 | `args` | por `kind` | conforme | — | `J0967` |
| 7 | `expires` | tempo | não | com relógio `synced`: > agora | `J0968` |

| `kind` | `args` | Faz | Papel equivalente | Versão |
|---|---|---|---|---|
| `confirm` | `confirm-args`: `seq` (1), `phase` (2), `hash` (3), `device` (4, fase `mcu`) | confirma a fase pendente, se for exatamente esta (seq, fase, hash) | `admin` | 1.0 |
| `reject` | `confirm-args` | rollback imediato da fase em confirmação (`J0969`) | `admin` | 1.0 |
| `report_full` | — | envia um relatório completo no próximo envio | `viewer` | 1.0 |
| `renew` | `renew-args`: `reason` (1) | renova o certificado já (§3.2) | — | 1.0 |
| `upload` | `upload-args` (§2.7.1) | carrega registos locais para o Deck | `viewer` | 1.0 |
| `session_open` | `session-open-args` (§2.12) | abre uma sessão remota | segundo a política | 1.3 |
| `session_close` | `session-close-args`: `id` (1), `reason` (2) | fecha uma sessão | — | 1.3 |

Um `confirm` ou `reject` cujo alvo não esteja em `confirming` responde `conflict` com `J0967` e não faz nada. Por isso um comando repetido, atrasado ou reordenado nunca estraga nada.

#### 2.7.1 `upload-args`

| # | Nome | Tipo | Obr. | Omissão | Validação |
|---|---|---|---|---|---|
| 1 | `upload` | ULID | sim | — | destino `PUT /agent/v1/uploads/{upload}` |
| 2 | `what` | `upload-kind` | sim | — | em `[agent.upload] allow` do nó, senão `forbidden` |
| 3 | `from` | tempo | não | início do registo | — |
| 4 | `to` | tempo | não | agora | — |
| 5 | `filter` | tstr | não | tudo | seletor de caminhos (documento 02, §9) |
| 6 | `max_bytes` | uint | não | `[agent.upload] max_bytes` | ≤ esse valor |

Formato do corpo: `decisions` e `events` em NDJSON (uma decisão ou evento por linha, na forma JSON deste documento), `logs` em NDJSON dos logs estruturados, `trace` no formato `.jktrace` (documento 16, §7). O carregamento vai na classe `bulk` do orçamento de uplink (documento 01, §13).

### 2.8 Hello e long-poll

`hello` (nó → Deck), no início de cada ligação:

| # | Nome | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `v` | `1` | sim | — |
| 2 | `node` | nome | sim | — |
| 3 | `org` | nome | sim | — |
| 4 | `proto` | `[major, minor]` | sim | `[1, N]`: a minor mais alta que o nó fala |
| 5 | `runtime` | semver | sim | versão em execução |
| 6 | `boot` | ULID | sim | gerado a cada arranque do `jokerd` |
| 7 | `uptime` | duração | sim | — |
| 8 | `hw` | digest | sim | impressão do hardware |
| 9 | `key_storage` | enum | sim | — |
| 10 | `time` | tempo | sim | relógio do nó |
| 11 | `clock` | `clock_quality` | sim | — |
| 12 | `desired_seq` | seq | sim | último DSD aceite (0 = nenhum) |
| 13 | `report_seq` | seq | sim | último relatório enviado |
| 14 | `event_seq` | seq | sim | último evento emitido |
| 15 | `trust_version` | seq | sim | versão do bundle instalado |
| 16 | `capabilities` | `[* capability]` | não | — |
| 17 | `kind` | `node-kind` | não, `linux` | — |

`hello-reply`: `proto_minor` (1, = min(nó, Deck)), `deck` (2), `time` (3, relógio do Deck), `event_ack` (4, último evento guardado), `need_full` (5), `settings` (6), `trust` (7, cadeia de bundles mais novos), `status` (8, `quarantined` ou `revoked`), `code` (9).

**`desired_seq` no hello** serve ao Deck para nunca emitir um `seq` que o nó recusaria: o próximo `seq` é sempre > max(o do Deck, o do nó). É o que salva um Deck reposto de uma cópia de segurança antiga (§4, S10).

`poll-reply` (resposta do long-poll, §3.11.1): `time` (1), `desired` (2, o DSD atual se `seq` > o pedido), `commands` (3, ≤ 16), `cursor` (4, opaco, para o próximo pedido), `event_ack` (5), `need_full` (6), `backoff` (7, espera imposta pelo Deck).

### 2.9 `agent-settings` e margens do nó

O Deck sugere; o nó aplica o valor limitado às margens abaixo (e às do `[agent]` local, que ganham).

| # | Nome | Omissão | Margem | Significado |
|---|---|---|---|---|
| 1 | `poll_wait` | `55s` | 10 s–300 s | duração máxima de um long-poll |
| 2 | `poll_interval` | `0s` (long-poll contínuo) | 0 ou 1 min–24 h | intervalo entre polls curtos (LTE) |
| 3 | `report_min_interval` | `10s` | 1 s–1 h | intervalo mínimo entre deltas |
| 4 | `full_interval` | `6h` | 10 min–7 d | relatório completo periódico |
| 5 | `heartbeat` | `5m` | 30 s–1 h | delta mínimo com `resources` se nada mudou |
| 6 | `event_batch` | `100` | 1–1 000 | eventos por lote |
| 7 | `drain_rate` | `50` | 1–1 000 | eventos/s ao esvaziar uma fila acumulada |

### 2.10 Estado reportado (`report`)

| # | Nome | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `v` | `1` | sim | — |
| 2 | `node` | nome | sim | — |
| 3 | `seq` | seq | sim | crescente por nó, persistido |
| 4 | `base` | seq | não | presente: é um **delta** (merge patch, RFC 7386) sobre o relatório `base`; ausente: completo |
| 5 | `time` | tempo | sim | — |
| 6 | `clock` | `clock_quality` | sim | — |
| 7 | `body` | `report-body` | sim | — |

`report-ack`: `ack` (1, o `seq` aceite) e `need_full` (2).

#### 2.10.1 `report-body`

Os mapas por nome (`devices`, `alarms`…) são chaveados pelo caminho ou nome do documento 02, §9, para que o merge patch funcione. Num delta, `null` apaga.

| # | Nome | Conteúdo | Completo inclui | Perfil micro |
|---|---|---|---|---|
| 1 | `node` | `board` (1), `arch` (2), `os` (3), `mode` (4), `site` (5), `tz` (6), `kind` (7), `boot` (8), `booted` (9) | tudo | `board`, `kind`, `boot` |
| 2 | `runtime` | `version` (1), `slot` (2), `previous` (3), `state` (4, `generation_state` da fase runtime), `proto_minor` (5), `worlds` (6) | tudo | `version` (= firmware), `state` |
| 3 | `generation` | `confirmed` (1), `running` (2), `state` (3), `previous` (4), `origin` (5, `gen-origin`), `confirm_by` (6) | tudo | `confirmed`, `running`, `state` |
| 4 | `desired` | `seq` (1), `state` (2, `apply-state`), `phase` (3), `wait` (4), `code` (5), `causes` (6, ≤ 16), `fetched` (7, `[feito, total]` em bytes), `next` (8), `rollout` (9) | tudo | `seq`, `state`, `code` |
| 5 | `drift` | nome → `code` (1), `since` (2), `detail` (3): deriva do lock (`J0401`), geração alterada localmente (`J0966`), ficheiros alterados em `/var/lib/joker` | tudo | — |
| 6 | `health` | `overall` (1, `health`), `codes` (2) | tudo | tudo |
| 7 | `groups` | grupo → `state` (1), `period` (2), `jitter_p999` (3), `overruns` (4, desde o início da geração) | tudo | `state` |
| 8 | `devices` | dispositivo → `link` (1, `link-state`, os seis valores da ref/00 §6), `since` (2), `driver` (3), `firmware` (4) | tudo | `link` |
| 9 | `instances` | instância → `health` (1), `class` (2), `failed` (3, membros) | tudo | — |
| 10 | `apps` | app → `state` (1), `version` (2), `digest` (3), `restarts` (4) | tudo | `state` |
| 11 | `alarms` | alarme → `state` (1), `severity` (2), `since` (3), `shelved_until` (4), `message` (5, ≤ 256 B); só alarmes com estado ≠ `normal` | tudo | ≤ 8, por gravidade |
| 12 | `trips` | saída → `since` (1), `cause` (2) | tudo | tudo |
| 13 | `overrides` | saída → `level` (1), `by` (2), `until` (3), `reason` (4): escritas manuais ativas | tudo | — |
| 14 | `resources` | §2.10.2 | tudo | micro |
| 15 | `mcus` | dispositivo → `link` (1), `board` (2), `firmware` (3), `firmware_digest` (4), `generation` (5, o `source_hash`), `state` (6, `generation_state`, com o estado de confirmação), `abi` (7), `link_version` (8, a negociada), `boot_count` (9), `reset_reason` (10), `counter` (11, `security_version`), `descriptor` (12, digest do `firmware.jkfw`), `gen_seq` (13), `session_mode` (14, ref/00 §6), `confirm_left` (15, tempo que falta na janela de confirmação), `params` (16, parâmetros alterados localmente: deriva, ref/07 §6.4 chave 16), `drivers` (17, o resumo dos descritores dos drivers, ref/07 §6.4 chave 15) | tudo | — |
| 16 | `identity` | `serial` (1), `not_after` (2), `key_storage` (3), `trust_version` (4), `floors` (5, papel → seq) | tudo | `not_after`, `trust_version` |
| 17 | `agent` | `transport` (1), `queue` (2), `queue_bytes` (3), `dropped` (4), `oldest` (5), `paused_until` (6), `hold` (7) | tudo | `queue`, `dropped` |
| 18 | `truncated` | chaves de `report-body` que foram cortadas (§3.8.3) | se houver | se houver |
| 19 | — | reservado: intenções pendentes (M18) | — | — |
| 20 | — | reservado: modelos e sombra (1.1) | — | — |
| 21 | `secrets` | segredo → `rev` (1), `ok` (2, decifrado e verificado) — nomes e revisões, **nunca** valores (ref/00 §9) | tudo | tudo |
| 22 | `drivers` | o manifesto de drivers do binário (ref/06 §3.8): driver → `version` (1), `tier` (2), `descriptor` (3, digest do descritor) | tudo | — |

`gen-ref`, usado em `generation` e nos eventos `apply`: `gen` (1, o inteiro do nó), `hash` (2), `at` (3, quando foi trocada ou confirmada), `by` (4, principal: `agent`, `user:<id>`, `deck:<deck_id>/user:<id>`), `label` (5).

**`health.overall`**, calculado pelo nó:

| Valor | Quando |
|---|---|
| `failed` | um grupo `stopped` ou `tripped` por overrun; uma app em `crashloop` ou `failed`; a última aplicação terminou em `failed`; o nó não arrancou a geração confirmada |
| `degraded` | um dispositivo `down`, `degraded` ou `drift`; uma instância não `ok`; um trip ativo; um alarme `critical` ativo; relógio `unsynced`; deriva |
| `ok` | nenhum dos anteriores |
| `unknown` | ainda a arrancar |

`health.codes` lista os J-codes que justificam o valor (por exemplo `J0401`).

#### 2.10.2 `resources`

| # | Nome | Tipo | Unidade | Perfil |
|---|---|---|---|---|
| 1 | `cpu` | pct | % média de 1 min | Linux |
| 2 | `mem_used` | uint | B | Linux |
| 3 | `mem_total` | uint | B | Linux |
| 4 | `disk_free` | uint | B, em `/var/lib/joker` | Linux |
| 5 | `disk_total` | uint | B | Linux |
| 6 | `wear` | pct | desgaste estimado do eMMC (documento 08, rodada 5 (e)) | Linux |
| 7 | `temp` | float | °C | ambos |
| 8 | `throttled` | bool | — | Linux |
| 9 | `store_lag` | duração | atraso do cursor mais lento do historian | Linux |
| 10 | `heap_free` | uint | B | micro |
| 11 | `heap_min` | uint | B, mínimo desde o arranque | micro |
| 12 | `rssi` | int | dBm | micro |
| 13 | `boot_count` | uint | — | ambos |
| 14 | `reset_reason` | tstr | — | micro |
| 15 | `uplinks` | uplink → `lag` (1), `budget_used` (2, %) | — | Linux |

#### 2.10.3 Exemplo de relatório completo (JSON)

```json
{
  "v": 1,
  "node": "hb-01234",
  "seq": 90211,
  "time": "2026-09-24T10:00:00.120Z",
  "clock": "synced",
  "body": {
    "node": { "board": "cm4-din", "arch": "aarch64", "mode": "prod", "site": "hb-01234", "kind": "linux" },
    "runtime": { "version": "1.0.3", "slot": "b", "previous": "1.0.2", "state": "confirmed", "proto_minor": 0 },
    "generation": {
      "confirmed": { "gen": 37, "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5", "at": "2026-09-24T02:41:10Z", "by": "agent" },
      "running": { "gen": 37, "hash": "sha256:4c1d0e8f6a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5" },
      "state": "confirmed",
      "origin": "agent"
    },
    "desired": { "seq": 418, "state": "confirmed" },
    "health": { "overall": "degraded", "codes": ["J0401"] },
    "groups": { "main": { "state": "ok", "period": "10s", "jitter_p999": "1.8ms", "overruns": 0 } },
    "devices": { "hp": { "link": "up" }, "heatmeter": { "link": "up" }, "pv": { "link": "drift" } },
    "apps": { "hp_opt": { "state": "running", "version": "3.2.0" }, "grid_limit": { "state": "running", "version": "1.1.0" } },
    "alarms": {},
    "trips": {},
    "overrides": {},
    "resources": { "cpu": 7.5, "mem_used": 402653184, "mem_total": 2147483648, "disk_free": 9663676416, "wear": 3, "temp": 51.0 },
    "identity": { "not_after": "2027-06-01T00:00:00Z", "key_storage": "file", "trust_version": 3 },
    "agent": { "transport": "https", "queue": 0, "dropped": 0 }
  }
}
```

### 2.11 Eventos

`event-batch`: `node` (1), `events` (2, ≤ `event_batch`). `event-ack`: `ack` (1).

`event`:

| # | Nome | Tipo | Obr. | Significado |
|---|---|---|---|---|
| 1 | `seq` | seq | sim | crescente, contíguo, persistido; um evento perdido mantém o seu `seq` (§3.9.3) |
| 2 | `id` | ULID | sim | identificador global, estável entre reenvios |
| 3 | `ts` | tempo | sim | UTC do nó no momento do evento |
| 4 | `clock` | `clock_quality` | sim | qualidade do relógio nesse momento |
| 5 | `boot` | ULID | sim | arranque em que o evento aconteceu |
| 6 | `mono` | uint | sim | ns monotónicos desde esse arranque (par de `ts`, ref/00, §3) |
| 7 | `type` | `event-type` | sim | — |
| 8 | `body` | por `type` | sim | — |

| `type` | Corpo | Classe na fila (§3.9.3) |
|---|---|---|
| `alarm` | `name` (1), `from` (2), `to` (3), `severity` (4), `message` (5), `by` (6), `shelved_until` (7) | A |
| `decision` | `output` (1), `writer` (2), `outcome` (3), `reason` (4), `level` (5), `value` (6), `applied` (7), `ttl` (8), `why` (9), `session` (10), `count` (11), `tick` (12) | B |
| `apply` | `seq` (1, do DSD), `state` (2), `phase` (3), `gen` (4), `device` (5), `code` (6), `causes` (7), `rollout` (8) | A |
| `trip` | `output` (1), `action` (2, `trip-action`), `cause` (3), `by` (4) | A |
| `security` | `kind` (1), `code` (2), `detail` (3), `kid` (4), `seq` (5) | A |
| `session` | `id` (1), `action` (2), `user` (3), `role` (4), `code` (5), `requests` (6), `writes` (7) | A |
| `gap` | `ranges` (1, `[[de, até], …]`, ≤ 64), `counts` (2, `event-type` → n) | nunca se perde |
| `clock` | `boot` (1), `mono` (2), `utc` (3), `quality` (4), `source` (5) | B |
| `command_result` | `id` (1), `status` (2), `code` (3) | B |
| `cert` | `action` (1), `not_after` (2), `serial` (3) | A |
| `agent` | `action` (1), `by` (2), `until` (3), `detail` (4) | C |

Os valores em `decision.value`/`applied` seguem a ref/00, §4.1, na unidade do sinal. `writer` usa as formas da ref/00, §5, mais `user:deck:<deck_id>/user:<id>` para escritas em sessões remotas (o escritor `user:<principal>` da ref/00 §5, com o principal da sessão).

**Que decisões vão para o Deck** (`[agent.audit] decisions`, §2.17). O registo de decisões local guarda **todas** (M3); o Deck só recebe as marcadas:

| Valor | O que envia |
|---|---|
| `manual` | todas as escritas, libertações e chamadas de métodos de escritores `manual` |
| `session` | todas as escritas feitas em sessões remotas (**sempre ativo**, não se desliga) |
| `trip`, `reset` | trips e resets (também como eventos `trip`) |
| `rejected` | transições para `rejected` ou `tripped` por (saída, escritor, motivo): a primeira, e depois uma com `count` quando o resultado muda ou a cada hora |
| `bridge` | escritas de pontes, agregadas como `rejected` |
| `action` | ações de dispositivos e instâncias (M19) |
| `intent` | reservado (M18, 1.1) |

Omissão: `["manual", "session", "trip", "reset", "rejected", "action"]`.

### 2.12 Sessão remota (agent/1.3)

`session-open-args`:

| # | Nome | Tipo | Obr. | Validação | Erro |
|---|---|---|---|---|---|
| 1 | `id` | ULID | sim | nova | — |
| 2 | `user` | `user-claim`: `id` (1), `name` (2), `groups` (3, ≤ 32), `mfa` (4) | sim | o Deck atesta a identidade; o nó decide o papel | — |
| 3 | `role` | `role` | sim | ≤ papel concedido (§3.10) | `J0981` |
| 4 | `reason` | tstr 1–256 B | sim | — | — |
| 5 | `ttl` | duração | sim | ≤ `[agent.remote] max_ttl` | `J0983` |
| 6 | `tunnel` | tstr | não | `wss://` de um dos `deck_urls`; omissão `<deck>/agent/v1/sessions/{id}/tunnel` | `J0985` |

**Túnel.** Uma ligação WebSocket (RFC 6455) **aberta pelo nó**, com mTLS. Cada mensagem binária é um `tunnel-frame` em CBOR: `[stream, kind, payload?]`.

| `kind` | `payload` | Sentido | Significado |
|---|---|---|---|
| `open` | uint: destino (0 = API local; outros reservados) | Deck → nó | abre um fluxo de bytes para a API local |
| `data` | bstr ≤ 64 KiB | ↔ | bytes do fluxo |
| `close` | — | ↔ | fecho ordenado |
| `reset` | uint: `agent-status` | ↔ | fecho abrupto |
| `credit` | uint: bytes | ↔ | controlo de fluxo por crédito (inicial: 256 KiB por fluxo) |
| `ping` | — | ↔ | vida; sem resposta em 30 s fecha o túnel |

Cada fluxo é **uma ligação à API local** (ref/09), como se viesse do socket Unix, mas com o principal pré-autenticado `deck:<deck_id>/user:<user.id>` (ref/00 §5) e o papel efetivo da sessão. HTTP/1.1, HTTP/2 e o WebSocket da API passam tal qual. A `jk --node deck:<nó>` e o Studio são clientes desta API.

### 2.13 HTTPS: endpoints do Deck

Base: um dos `deck_urls` do bundle, com o sufixo `/agent/v1`. TLS 1.3 (1.2 aceite no perfil micro), HTTP/2 preferido, mTLS em tudo exceto `/enroll`. `Content-Type: application/json` por omissão; `application/cbor` se negociado; `application/cose` para um documento assinado isolado.

| Método e caminho | Autenticação | Pedido | Resposta | Notas |
|---|---|---|---|---|
| `POST /enroll` | token no corpo | `enroll-request` | `enroll-reply` | sem mTLS |
| `GET /enroll/nonce` | nenhuma | — | `{"nonce": <bytes>}` | nonce de 32 B para a atestação (1.3); sem mTLS |
| `POST /renew` | mTLS, aceita certificado expirado até `renew_grace` | `renew-request` | `renew-reply` | — |
| `GET /trust?from=<versão>` | mTLS | — | `[+ cose]` | cadeia de bundles a partir de `from` |
| `POST /hello` | mTLS | `hello` | `hello-reply` | no início de cada ligação |
| `GET /poll?desired=<seq>&cursor=<c>&wait=<d>` | mTLS | — | `poll-reply` | long-poll (§3.11.1); responde logo se houver novidade |
| `POST /report` | mTLS | `report` | `report-ack` | `409 need_full` se a base não existir |
| `POST /events` | mTLS | `event-batch` | `event-ack` | — |
| `POST /registry-token` | mTLS | `{"scope": [...]}` | `registry-token`: `token` (1), `expires` (2), `scope` (3) | token *bearer* para as fontes `auth = token` |
| `GET /blobs/{digest}` | mTLS | — | bytes | proxy de artefactos (`kind = deck`); aceita `Range` |
| `PUT /uploads/{id}` | mTLS | corpo do §2.7.1, `Content-Encoding: zstd` opcional | `status-reply` | só para um `upload` pedido e ainda aberto |
| `GET /sessions/{id}/tunnel` | mTLS + WebSocket | — | `tunnel-frame` | 1.3 |

Cabeçalhos: o nó envia `User-Agent: jokerd/<versão> agent/1.<minor>`; o Deck responde com `Date` e, nos erros, `Retry-After`.

### 2.14 MQTT: tópicos

MQTT 5 (3.1.1 aceite no perfil micro), sobre TLS com mTLS. Identificador de cliente: `<org>.<nó>`. Prefixo: `jk/1/<org>/<nó>/`.

| Sufixo | Sentido | QoS | Retain | Conteúdo |
|---|---|---|---|---|
| `hello` | nó → Deck | 1 | não | COSE(`hello`) |
| `hello/reply` | Deck → nó | 1 | não | COSE(`hello-reply`) |
| `desired` | Deck → nó | 1 | **sim** | o DSD (já é COSE) |
| `cmd` | Deck → nó | 1 | não | comando (já é COSE); *message expiry* = `expires` |
| `report` | nó → Deck | 1 | não | COSE(`report`) |
| `ack/report` | Deck → nó | 1 | não | COSE(`report-ack`) |
| `events` | nó → Deck | 1 | não | COSE(`event-batch`) |
| `ack/events` | Deck → nó | 1 | não | COSE(`event-ack`) |
| `status` | nó → Deck | 1 | **sim** | COSE(`{1: online bool, 2: boot, 3: time}`); também como *will* com `online = false`, assinado no momento da ligação |
| `blob/req` | nó → Deck | 1 | não | COSE(`blob-request`), só perfil micro |
| `blob/data` | Deck → nó | 1 | não | `blob-chunk` em CBOR, sem assinatura (a integridade vem do digest) |

| Parâmetro | Linux | Micro |
|---|---|---|
| `clean start` | `false` | `false` |
| *session expiry* | `7d` (`[agent.mqtt] session_expiry`) | `1d` |
| *keepalive* | `60s`; `5m`–`20m` em LTE | `120s` |
| tamanho máximo de pacote | 256 KiB | 4 KiB |
| *receive maximum* | 16 | 2 |

**ACL do broker** (obrigatória, verificada pelo `jk doctor` quando possível): um nó só publica em `jk/1/<org>/<nó>/{hello,report,events,status,blob/req}` e só subscreve `jk/1/<org>/<nó>/{hello/reply,desired,cmd,ack/#,blob/data}`, com `<nó>` igual ao `CN` do certificado. Uma publicação recusada pelo broker aparece localmente como `J0977`.

As sessões remotas não passam por MQTT (`J0985`).

### 2.15 Perfil micro

Para um Joker Micro autónomo (M2, com rede). O mesmo CDDL, com as restrições:

| Mensagem | Chaves permitidas | Tamanho máximo |
|---|---|---|
| `desired-state` | 1–10, 12, 14 (sem `tz` ≠ `UTC`), 16, 17, 18, 20, 22 | 2 KiB |
| `runtime-target` | 1–4; `artifact` do tipo `joker.micro-firmware.v1` | — |
| `generation-target` | 1, 2; `config` do tipo `joker.generation.v1` | — |
| `command` | `confirm`, `reject`, `report_full`, `renew` | 512 B |
| `report` | as da coluna "perfil micro" do §2.10.1 | 1 KiB |
| `event-batch` | tipos `alarm`, `apply`, `trip`, `security`, `gap`, `clock`, `cert` | 1 KiB, ≤ 8 eventos |
| `hello`, `hello-reply` | todas | 512 B |

Uma chave fora do subconjunto listada em `crit` é `J0990`; fora de `crit`, é ignorada. Uma janela com `tz` ≠ `UTC` é `J0991`.

**CoAP** (RFC 7252) sobre DTLS 1.2 com certificado, base `coaps://<host>/a1/`:

| Recurso | Método | Pedido | Resposta | Notas |
|---|---|---|---|---|
| `/a1/h` | POST | `hello` | `hello-reply` | — |
| `/a1/d?s=<seq>` | GET + Observe (RFC 7641) | — | DSD (Content-Format 18, COSE_Sign1) | notificação a cada DSD novo |
| `/a1/c?k=<cursor>` | GET + Observe | — | `[* cose]` | comandos |
| `/a1/r` | POST | `report` | `report-ack` | — |
| `/a1/e` | POST | `event-batch` | `event-ack` | — |
| `/a1/b/<digest em hex>` | GET + Block2 (RFC 7959), blocos de 512 B ou 1 KiB | — | bytes | artefactos |
| `/a1/k` | POST | `renew-request` | `renew-reply` | — |

Content-Format 60 (`application/cbor`) para as mensagens não assinadas. Com DTLS terminado no Deck, as mensagens do nó não vão em COSE (como em HTTPS).

**Artefactos no perfil micro**, por ordem de preferência: HTTPS com `Range` (OTA HTTPS do ESP-IDF ou equivalente), CoAP Block2, ou MQTT `blob/req` + `blob/data` em pedaços de ≤ 2 KiB. O MCU pede um pedaço de cada vez; a descarga retoma pelo `offset`.

| Mensagem | Chaves |
|---|---|
| `blob-request` | `digest` (1), `offset` (2, bytes), `length` (3, ≤ 2 048) |
| `blob-chunk` | `digest` (1), `offset` (2), `data` (3, ≤ `length` pedido), `total` (4, tamanho do artefacto, no primeiro pedaço) |

Um pedaço com `digest` ou `offset` que o MCU não pediu é ignorado. O digest do artefacto inteiro é verificado no fim, e a assinatura antes de gravar a imagem como arrancável.

`status` (MQTT): `online` (1, bool), `boot` (2), `time` (3). `registry-token` é pedido com `{"scope": ["repository:<nome>:pull", …]}` (`registry-token-request`, `scope` (1)).

### 2.16 Tipos de artefacto e papéis de assinatura

| Tipo de media (forma curta da ref/00) | Papel que assina | Fase | Versão |
|---|---|---|---|
| `joker.runtime.v1` | `runtime` | `runtime` | 1.0 (tipo novo, §5, D11) |
| `joker.config.v1+cbor` | `config` | `generation` | 1.0 (tipo novo, §5, D11) |
| `joker.app.v1+wasm` | `apps` | `generation` | 1.0 |
| `joker.profile.v1+toml` | `profiles` | `generation` | 1.0 |
| `joker.board.v1+toml` | `boards` | `generation` | 1.0 |
| `joker.class.v1+jkc` | `classes` | `generation` | 1.0 |
| `joker.micro-firmware.v1` | `firmware` | `mcu` ou `runtime` (micro) | 1.0 |
| `joker.generation.v1` | `config` | `mcu` ou `generation` (micro) | 1.0 |
| `joker.driver.v1+wasm`, `joker.driver.v1+tar` | `drivers` | `generation` | 1.0 |
| `joker.lock.v1+toml` | (dentro da geração: coberto pelo `config`) | `generation` | 1.0 |
| `joker.secrets.v1` | `secrets` (por entrada) | chave 22; não é uma fase | 1.0 |
| `joker.model.v1+jkm` | `models` | `generation` | **reservado 1.1**: um nó 1.0 recusa com `J0957` |

Um artefacto é aceite se tiver pelo menos uma assinatura válida de uma chave do bundle com o papel da tabela, e se o manifesto `[security] trust` não o excluir. Um artefacto sem assinatura só é aceite se a geração alvo tiver `mode = "dev"`, o bundle tiver `min_mode = dev` **e** o `node.toml` tiver `[policy] allow_dev = true` (§3.3, V19; ref/02 §3.5.1). O DSD, os comandos, os bundles e os pisos são sempre assinados, em qualquer modo.

Os tipos de media e os papéis são os da tabela única da ref/00 §7.1 (D13); esta tabela só diz em que fase entram. No registo OCI, o `mediaType`/`artifactType` completo é `application/vnd.joker.<resto>` (por exemplo `application/vnd.joker.app.v1+wasm`).

### 2.17 Configuração do agente no nó

A configuração de arranque do nó é da ref/02 (`/etc/joker/node.toml`), que acolhe esta secção `[agent]` tal qual; o significado das chaves define-se aqui:

```toml
[agent]
enabled             = true                 # omissão: true se o bundle tiver authority
transport           = "https"              # https | mqtt | coap (micro)
urls                = []                   # sobrepõe deck_urls do bundle (mesma org)
poll                = "long"               # long | duração (p. ex. "15m" em LTE)
backoff_max         = "15m"
offline_warn        = "7d"
local_changes       = "hold"               # hold | overwrite
max_applies_per_day = 6
mirrors             = ["https://mirror.example.com"]
cbor                = false                # CBOR sobre HTTPS (poupança em LTE)
maintenance         = [{ days = [1, 2, 3, 4, 5], start = "01:00", length = "4h", tz = "Europe/Lisbon" }]

[agent.queue]
max_disk = "64 MiB"

[agent.report]
min_interval  = "10s"
full_interval = "6h"
heartbeat     = "5m"

[agent.audit]
decisions = ["manual", "session", "trip", "reset", "rejected", "action"]

[agent.upload]
allow     = ["decisions", "logs", "events"]
max_bytes = "64 MiB"

[agent.mqtt]
url            = "mqtts://mqtt.example.com:8883"
keepalive      = "60s"
session_expiry = "7d"

[agent.remote]                              # agent/1.3
allow           = false
max_role        = "operator"                # viewer | operator | admin
default_role    = ""                        # papel para utilizadores do Deck sem mapeamento; vazio = recusar
consent         = "none"                    # none | local
consent_signal  = ""                        # sinal bool que autoriza (p. ex. um comutador "manutenção remota")
consent_timeout = "5m"
max_sessions    = 2
max_ttl         = "8h"
idle            = "15m"
max_rate        = "1 MiB/s"
```

| Chave | Tipo | Omissão | Validação | Significado |
|---|---|---|---|---|
| `enabled` | bool | ver acima | — | desliga o agente sem desenrolar |
| `transport` | enum | `https` | `coap` só em micro | — |
| `urls` | lista de URLs | `[]` | mesmo esquema do bundle | — |
| `poll` | `long` ou duração | `long` | 1 min–24 h | `poll_interval` local (ganha ao do Deck) |
| `backoff_max` | duração | `15m` | 1 min–6 h | teto do backoff (§3.11.4) |
| `offline_warn` | duração | `7d` | ≥ 1 h | alarme `joker.agent.offline` |
| `local_changes` | enum | `hold` | — | §3.7 |
| `max_applies_per_day` | uint | 6 | 1–100 | trocas por 24 h deslizantes, de qualquer fase |
| `mirrors` | lista | `[]` | ≤ 4 | fontes tentadas antes das do DSD |
| `cbor` | bool | `false` | — | — |
| `maintenance` | lista de janelas | sem restrição | ≤ 16 | interseção com as do DSD |
| `queue.max_disk` | tamanho | `64 MiB` | 1 MiB–4 GiB | teto da fila de eventos |
| `report.*` | durações | §2.9 | margens do §2.9 | ganham às do Deck |
| `audit.decisions` | lista | acima | valores do §2.11 | `session` é sempre incluído |
| `upload.allow` | lista | `["decisions", "logs", "events"]` | `upload-kind` | `trace` tem de ser pedido explicitamente |
| `upload.max_bytes` | tamanho | `64 MiB` | ≤ 1 GiB | — |
| `mqtt.*` | — | acima | — | §2.14 |
| `remote.*` | — | acima | §3.10 | — |

As chaves são de ficheiro escrito por pessoas: uma chave desconhecida é `J0140` (ref/00, §1). Nenhuma chave `[agent]` chega pelo protocolo: o Deck não a consegue mudar.

### 2.18 Sinais, alarmes e comandos locais

Sinais do nó (prefixo reservado `joker.`, gravados e alarmáveis como os outros):

| Sinal | Tipo | Unidade | Significado |
|---|---|---|---|
| `joker.agent.connected` | bool | — | última troca com o Deck bem-sucedida há menos de 3 × `poll_wait` |
| `joker.agent.offline_for` | f64 | s | tempo desde a última troca bem-sucedida |
| `joker.agent.queue` | u64 | — | eventos por confirmar |
| `joker.agent.dropped` | u64 | — | eventos perdidos desde o enrolamento |
| `joker.agent.desired_seq` | u64 | — | último DSD aceite |
| `joker.agent.cert_days` | f64 | d | dias até ao fim do certificado |

Alarmes do nó:

| Alarme | Gravidade | Quando |
|---|---|---|
| `joker.agent.offline` | `medium` | `offline_for` > `offline_warn` (`J0974`) |
| `joker.agent.cert_expiring` | `medium` | menos de 1/3 da validade e renovação falhada (`J0935`) |
| `joker.agent.cert_expired` | `high` | certificado expirado |
| `joker.agent.revoked` | `high` | Deck respondeu `revoked` (`J0934`) |
| `joker.agent.quarantined` | `high` | Deck respondeu `quarantined` (`J0937`, `J0938`) |
| `joker.agent.refused` | `high` | DSD, comando, bundle ou piso recusado por segurança (`J0939`–`J0945`, `J0951`) |
| `joker.agent.revoked_artifact` | `high` | um artefacto em execução está num piso `revoked` |
| `joker.agent.queue_overflow` | `medium` | eventos perdidos (`J0970`) |
| `joker.agent.hold` | `low` | desejado em espera por alterações locais ou pausa |
| `joker.agent.session` | `low` | há uma sessão remota aberta |
| `joker.agent.session_request` | `medium` | pedido de sessão à espera de consentimento local |

Comandos `jk` (documento 07; todos falam com a API local, ref/09 §5.21):

| Comando | Papel | Faz | API local (ref/09 §5.2, §5.21) |
|---|---|---|---|
| `jk node enroll <alvo> --ca <dir>` | `admin` | enrola sem Deck, com uma CA local (§3.1.1) | `createEnrollmentCsr`, `installEnrollmentCertificate` |
| `jk node enroll <alvo> --deck <url> --token <t>` | `admin` | enrola num Deck (§3.1.2) | `enrollWithDeck` |
| `jk node enroll <mcu> --link <dispositivo>` | `admin` | enrola um MCU atrás do nó e provisiona a `K_link` (ref/07 §4.7) | — (local, por sonda ou USB; o nó recebe o segredo por `pushSecrets`) |
| `jk node renew` | `admin` | renova o certificado já | `renewNode` |
| `jk node trust [show \| push <ficheiro.cose> \| set --anchors …]` | `viewer` / `admin` | mostra o bundle e as âncoras, instala um bundle, troca as âncoras (verificados como os outros) | `getNodeTrust`, `pushTrustBundle`, `setTrustAnchors` |
| `jk agent status` | `viewer` | ligação, autoridade, seq, fila, pausa, hold, sessões | `getAgent` |
| `jk agent desired [--json]` | `viewer` | o DSD atual, descodificado, e o estado da aplicação | `getAgentDesired` |
| `jk agent pause --ttl <d> --reason "…"` / `jk agent resume` | `admin` | suspende as trocas (o download e os relatórios continuam) | `pauseAgent`, `resumeAgent` |
| `jk agent import <dir>` | `admin` | aplica um DSD e artefactos trazidos à mão (§3.7.3) | uploads de blobs, `importAgentBundle` |
| `jk session list \| accept <id> \| close <id>` | `operator` | sessões remotas (1.3) | `listAgentSessions`, `acceptAgentSession`, `closeAgentSession` |

Nenhum destes endpoints se pode usar **dentro** de uma sessão remota (§3.10, `J0986`), exceto as leituras `getAgent` e `getAgentDesired`.

### 2.19 Limites

| Coisa | Linux | Micro |
|---|---|---|
| DSD | 64 KiB | 2 KiB |
| artefactos por DSD | 256 | 4 |
| MCUs por DSD | 32 | — |
| comando | 4 KiB | 512 B |
| comandos por `poll-reply` | 16 | 4 |
| bundle de confiança | 16 KiB, 32 chaves | 2 KiB, 8 chaves |
| relatório completo | 256 KiB (forma JSON) | 1 KiB |
| delta | 32 KiB | 512 B |
| lote de eventos | 256 KiB, ≤ 1 000 eventos | 1 KiB, ≤ 8 |
| corpo de um evento | 4 KiB | 256 B |
| `message` de alarme | 256 B | 64 B |
| fila de eventos | `queue.max_disk` (omissão 64 MiB) | 32 eventos em RAM (+ flash, se houver) |
| sessões simultâneas | `max_sessions` (≤ 4) | — |
| fluxos por túnel | 16 | — |
| upload | `upload.max_bytes` (≤ 1 GiB) | — |
| mensagem MQTT | 256 KiB | 4 KiB |

Uma mensagem acima do limite é `too_large` com `J0976`.

### 2.20 Códigos

Gama `J0930`–`J0999` (ref/00, §8: `J09xx`, frota). Gravidades do documento 16, §9.

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0930` | error | token de enrolamento inválido, expirado ou já usado |
| `J0931` | error | o bundle recebido no enrolamento não é assinado pela raiz indicada no token |
| `J0932` | error | nome de nó inválido, já usado na organização, ou diferente do CSR |
| `J0933` | warning | chave de identidade em ficheiro, sem TPM nem elemento seguro (ou atestação exigida em falta) |
| `J0934` | error | certificado do nó revogado |
| `J0935` | warning | certificado do nó perto do fim ou expirado; renovação pendente |
| `J0936` | error | renovação recusada: fora de `renew_grace` ou prova inválida |
| `J0937` | error | impressão de hardware diferente da do enrolamento: nó em quarentena |
| `J0938` | error | a mesma identidade está ligada a partir de dois nós: clone suspeito |
| `J0939` | error | bundle de confiança ou piso inválido: assinatura, raiz desconhecida ou versão não crescente |
| `J0940` | error | assinatura COSE inválida, algoritmo não aceite ou `kid` desconhecido |
| `J0941` | error | o documento não é da autoridade do nó (papel `deck` em falta ou outro Deck) |
| `J0942` | error | `seq` do estado desejado não é maior do que o último aceite |
| `J0943` | error | `seq` repetido com conteúdo diferente (dois emissores) |
| `J0944` | error | documento ou comando destinado a outro nó ou organização |
| `J0945` | error | documento expirado ou emitido no futuro (relógio sincronizado) |
| `J0946` | error | formato não suportado: versão, chave crítica desconhecida ou fonte inválida |
| `J0947` | error | downgrade não autorizado ou abaixo do piso de segurança |
| `J0948` | error | runtime incompatível com a placa, a arquitetura ou o estado local |
| `J0949` | error | lista de artefactos incoerente com a geração, o lock ou os MCUs |
| `J0950` | error | artefacto descarregado não confere com o digest ou o tamanho |
| `J0951` | error | estado desejado ou artefacto sem nenhuma assinatura aceite para o seu tipo (uma assinatura inválida é `J0940`) |
| `J0952` | error | artefacto não descarregado dentro de `fetch_deadline` |
| `J0953` | error | espaço em disco insuficiente para preparar a aplicação |
| `J0954` | error | geração recusada pela validação (causas em `causes`) |
| `J0955` | error | hash da geração calculado diferente do declarado |
| `J0956` | error | geração `mode = "dev"` recusada num nó `prod` |
| `J0957` | error | tipo de artefacto ou funcionalidade não suportado nesta versão |
| `J0958` | error | app compilada para um mundo WIT que a runtime alvo não aceita |
| `J0959` | warning | aplicação adiada (janela, relógio, pausa, alterações locais) |
| `J0960` | error | janela de confirmação expirou: rollback |
| `J0961` | error | saúde falhou durante a confirmação: rollback |
| `J0962` | warning | limite local de aplicações por dia atingido |
| `J0963` | error | firmware de MCU incompatível (placa, ABI, contador de segurança) |
| `J0964` | error | blob de geração do MCU recusado (causa em `causes`, p. ex. `J0710`) |
| `J0965` | error | atualização do MCU pelo Joker Link falhou ou não confirmou |
| `J0966` | warning | geração em execução alterada localmente: estado desejado em espera |
| `J0967` | error | comando sem alvo válido (fase não pendente ou hash diferente) |
| `J0968` | error | comando fora da janela de tempo ou expirado |
| `J0969` | error | geração rejeitada pelo Deck durante a confirmação: rollback |
| `J0970` | warning | fila de eventos cheia: eventos perdidos |
| `J0971` | warning | relatório truncado por exceder o limite |
| `J0972` | warning | relógio do nó desalinhado face ao Deck em mais de 5 min |
| `J0973` | warning | minor do protocolo do nó fora das suportadas pelo Deck (só núcleo mínimo) |
| `J0974` | warning | Deck inacessível há mais de `offline_warn` |
| `J0975` | error | certificado do Deck ou do broker não confere com os pinos do bundle |
| `J0976` | error | mensagem acima do limite do transporte ou do perfil |
| `J0977` | error | publicação MQTT recusada pelo broker (ACL) |
| `J0978` | error | mensagem mal formada ou com formas de chave misturadas |
| `J0979` | error | política de confirmação, calendário ou janela inválidos |
| `J0980` | error | sessões remotas desativadas neste nó |
| `J0981` | error | papel pedido acima do que o nó concede a este utilizador do Deck |
| `J0982` | error | limite de sessões simultâneas atingido |
| `J0983` | error | sessão expirada, fechada, ou `ttl` acima de `max_ttl` |
| `J0984` | error | consentimento local recusado ou não dado a tempo |
| `J0985` | error | transporte sem suporte a sessões remotas |
| `J0986` | error | pedido no túnel a um endpoint proibido em sessões remotas |
| `J0990` | error | perfil micro: chave crítica fora do subconjunto |
| `J0991` | error | perfil micro: janela com fuso horário diferente de UTC |
| `J0992` | error | estado desejado com geração recusado: `[policy] apply_from` do nó não inclui `deck` |

`J0988`–`J0989` e `J0993`–`J0999` ficam livres (o `J0987` foi atribuído pela ref/02, §2.6). Os códigos das entradas de segredos (`J0922`–`J0926`) e da política de modo (`J0906`) são os da ref/02.

---

## 3. Comportamento esperado

### 3.1 Enrolamento

#### 3.1.1 Sem Deck: `jk node enroll <alvo> --ca <dir>`

Para desenvolvimento, para a trilha e para quem não quer Deck (documento 20, D5).

```
jk (portátil)                              nó (jokerd)
  │ ssh + socket local /run/joker/api.sock   │
  │── pedir CSR ────────────────────────────▶│ gera a chave P-256 (TPM se existir)
  │◀─ CSR, hw, key_storage ─────────────────│
  │ assina o CSR com a CA de <dir>            │
  │ assina um bundle sem authority com a raiz de <dir>
  │── instalar cert + bundle ──────────────▶│ verifica: bundle assinado pela raiz; cert = chave
  │◀─ ok ───────────────────────────────────│ agente desligado (sem authority)
```

O diretório da CA tem `root.key` (Ed25519, a raiz), `ca.key`/`ca.crt` (P-256, a emissora), `org` (nome), `dev` (ficheiro vazio, só numa CA de desenvolvimento) e, depois do `jk ca bundle`, `trust.cose` (o bundle de confiança, ref/02 §2.5.5). O `jk` cria-o com `jk ca init <dir> --org <org>`, ou no `jk node enroll --ca <dir>` se não existir (com `--dev`). Se a pasta tiver `trust.cose` (o bundle do `jk ca bundle`, ref/02 §2.5.5), é esse que o enrolamento instala, em vez do bundle sem `authority` do diagrama. O `jk ca init` cria também o certificado de utilizador `admin-<utilizador>` (SAN `urn:joker:role:admin`) e regista-o em `[user]` do `config.toml` do cliente (ref/02 §3.3.3). O canal de arranque até ao nó é o da ref/02 §3.2.3: o socket local, com acesso físico ou por SSH (`--node ssh://…`; `createEnrollmentCsr` e `installEnrollmentCertificate`, ref/09 §5.2), com o código de uso único (`--code`) pelo mesmo socket quando o nó o mostra. **Nunca há uma porta de enrolamento aberta à rede**; no enrolamento pelo Deck (§3.1.2) é o nó que liga para fora, com o token. O CSR leva a chave KEM no SAN (§2.4.2), e a instalação escreve as âncoras e o primeiro bundle.

#### 3.1.2 Com Deck: `jk node enroll <alvo> --deck <url> --token <t>`, ou `jokerd enroll`

```
nó                                                     Deck
 │ gera a chave; CSR; hw                                 │
 │── POST /enroll {token, node, csr, hw, key_storage} ──▶│ valida o token (J0930), o nome (J0932)
 │                                                       │ emite o certificado; marca o token usado
 │◀─ {cert, trust, deck, settings} ─────────────────────│
 │ verifica: raiz do bundle tem o kid do token (J0931)   │
 │           cert é da chave gerada; scope inclui o nó   │
 │ instala; hello; relatório completo                    │
```

- A chave **nunca sai do nó**: é o nó que fala com o Deck, e o `jk` só lhe passa o URL e o token.
- **Sem toque** (*zero-touch*): a imagem do nó leva `jokerd enroll --deck <url> --token-file <f>` no primeiro arranque, com um token preso ao site. O token é um segredo e é apagado depois de usado.
- Um nó já enrolado recusa um novo enrolamento sem `--force`; com `--force`, gera uma chave nova (a identidade antiga fica órfã e o Deck revoga-a).
- Mudar de Deck é um bundle novo com outra `authority`, assinado pela raiz (§3.2.4), ou um novo enrolamento.

#### 3.1.3 MCU autónomo

O MCU gera a chave de identidade (elemento seguro, se houver) e a chave X25519 de segredos no primeiro arranque. O enrolamento é feito pela ferramenta ligada por USB ou pelo Joker Link (`jk node enroll mcu:<porta> --deck <url> --token <t>`): a `jk` pede o CSR ao MCU (com a chave KEM no SAN), faz o `POST /enroll` com `kind = micro`, e devolve ao MCU o certificado, as âncoras e o bundle, que o MCU verifica como no §3.1.2 e guarda na área `IDENTITY` (registos 18 e 19, ref/08 §2.10.2; bundle ≤ 2 KiB). Um MCU com HTTPS pode enrolar-se sozinho.

Um MCU **atrás de um nó** não tem certificado: fala com o nó pelo Joker Link, com a `K_link`, que `jk node enroll <mcu> --link <dispositivo>` gera e provisiona (ref/07 §4.7): no MCU, os registos 16 e 17 da área `IDENTITY`; no nó, o segredo `secret:link.<dispositivo>`.

### 3.2 Renovação, revogação e nós clonados

#### 3.2.1 Renovação

- A partir de **2/3 da validade**, o nó tenta renovar uma vez por dia, com chave nova (`renew-request`). Sucesso: evento `cert` `renewed`; o certificado antigo deixa de ser usado.
- Um comando `renew` antecipa a renovação.
- A renovação é atómica: a chave e o certificado novos só substituem os antigos depois do `renew-reply` guardado com fsync. Uma falha a meio deixa o par antigo.
- Com o certificado **expirado**, o nó continua a tentar `POST /renew` (o único endpoint que o Deck aceita nessas condições), até `renew_grace`. Depois disso, `J0936`: é preciso um novo enrolamento no local.
- Nada disto afeta o controlo. O nó controla com o certificado expirado, revogado ou sem nenhum.

#### 3.2.2 Revogação

- É uma decisão do Deck (a CA é dele). O Deck recusa o TLS ou responde `revoked` (`J0934`) em qualquer endpoint. O broker MQTT tem de consultar a lista de revogação do Deck (CRL ou OCSP da CA emissora); o `jk doctor` avisa se o broker não o fizer.
- O nó revogado pára o agente, levanta `joker.agent.revoked` e continua a funcionar no último estado confirmado. Um nó revogado nunca é "desligado" pelo Deck: o princípio 1 vale também aqui.
- Revogar uma **chave do bundle** (por exemplo, uma chave `deck` comprometida) é um bundle novo sem essa chave, assinado pela raiz. O Deck entrega-o, mas não o consegue fabricar.

#### 3.2.3 Nó clonado

| Caso | Deteção | Reação |
|---|---|---|
| chave em TPM ou elemento seguro | impossível clonar a chave; o clone não faz TLS | — |
| chave em ficheiro, cartão copiado para outra placa | `hello.hw` ≠ impressão do enrolamento | o Deck responde `quarantined` (`J0937`): aceita relatórios e eventos, marcados, e não entrega DSD nem comandos |
| duas placas com a mesma identidade ligadas ao mesmo tempo | dois `boot` diferentes com ligações sobrepostas, ou `event_seq` a andar para trás | as duas em quarentena (`J0938`) |

A **impressão do hardware** (`hw`) é o SHA-256 de `serial da placa ‖ serial do SoC ‖ MAC da interface primária ‖ chave pública EK do TPM (se houver)`, cada campo em UTF-8 e separado por `\n`. Uma troca legítima de hardware (reparação) resolve-se no Deck (aceitar a nova impressão) ou com um novo enrolamento. A quarentena nunca mexe no controlo do nó.

#### 3.2.4 Bundle e pisos

- O nó aceita bundles de três sítios: `enroll-reply`, `hello-reply`/`renew-reply`/`GET /trust` e o campo `trust` do DSD. A regra é sempre a do §2.4.3.
- Um bundle que muda a `authority` faz o nó esquecer o `seq` do Deck anterior: o próximo DSD da nova autoridade pode começar em 1. É a única forma de o `seq` recomeçar.
- Um piso novo aplica-se de imediato ao que ainda não foi trocado. Se o que **está em execução** estiver revogado, o nó levanta `joker.agent.revoked_artifact` e **continua a correr**: parar seria pior do que o risco, e a correção é um DSD novo.

### 3.3 Validação do estado desejado

O nó valida por esta ordem. O primeiro passo que falha decide o resultado.

| # | Passo | Falha | O `seq` fica consumido? |
|---|---|---|---|
| V1 | COSE bem formado, `alg` ∈ {−8, −7}, `content type` certo | `J0940` | não |
| V2 | `kid` no bundle instalado; `not_after` (se relógio `synced`) | `J0940` | não |
| V3 | a chave tem o papel `deck` e o seu `deck` = `authority` | `J0941` | não |
| V4 | assinatura válida | `J0940` | não |
| V5 | CBOR determinístico, `v` = 1, todas as chaves de `crit` conhecidas, limites do §2.19 | `J0946`, `J0976` | não |
| V6 | `node` e `org` são os deste nó | `J0944` | não |
| V7 | `seq` > último aceite. Igual e com o mesmo conteúdo: `noop`. Igual com conteúdo diferente: `J0943`. Menor: `J0942` | `J0942`, `J0943` | não |
| V8 | `issued` e `expires`, só com relógio `synced` | `J0945` | não |
| — | **aceite:** o DSD é guardado com fsync, o `seq` passa a ser o último aceite, evento `apply` `accepted`; um DSD anterior ainda não trocado passa a `superseded` | — | — |
| V9 | cada bundle em `trust`, por ordem (§2.4.3) | `J0939` | sim |
| V10 | cada piso em `floors` (§2.4.4) | `J0939` | sim |
| V10b | cada entrada de `secrets` (chave 22), pela ordem da ref/02 §3.4.3: assinatura do papel `secrets`, `wrap` para este nó, decifração de teste, `rev` ≥ à guardada; uma entrada que falha é recusada sozinha (evento `security`), as outras guardam-se, e um segredo rodado não cria geração | `J0922`–`J0926` | sim |
| V11 | todos os `media_type` conhecidos e suportados nesta versão | `J0957` | sim |
| V12 | `artifacts` coerente: sem duplicados; `runtime.artifact`, `generation.config` e os artefactos dos `mcus` presentes e com o tipo certo | `J0949` | sim |
| V13 | `confirm`, `schedule` e `window` válidos | `J0979` | sim |
| V14 | runtime: anotações do manifesto OCI (arquitetura, placa, `state_min`) | `J0948` | sim |
| V15 | runtime e firmwares: downgrade só com `allow_downgrade`; nunca abaixo de `min_version` nem num digest `revoked` | `J0947` | sim |
| V16 | espaço: soma dos `size` dos artefactos que faltam + reserva (max(10% do disco, 256 MiB)) ≤ livre | `J0953` | sim |
| V17 | **descarga** (estado `fetching`): o `joker.config.v1` primeiro; depois os outros, no máximo 2 em paralelo; cada um com tamanho e digest (`J0950`) e assinatura do papel certo (`J0951`) | `J0950`, `J0951`, `J0952` | sim |
| V18 | **staging**: os digests do lock, os dos `mcus` e os de `artifacts` coincidem (exceto runtime e configuração); cada `mcu-target.device` existe com o driver `joker-link` | `J0949` | sim |
| V19 | a origem `deck` está em `[policy] apply_from` (ref/02 §3.5.1); `mode` da geração ≥ `min_mode` do bundle e, se for `dev`, `[policy] allow_dev = true`; em `prod`, todos os artefactos assinados | `J0992`, `J0956`, `J0906`, `J0951` | sim |
| V20 | a validação do `jk check`, com os mesmos crates, para a runtime **alvo** (a nova, se houver fase `runtime`), incluindo o `[security] trust` do manifesto (§2.4.3) | `J0954` + causas | sim |
| V21 | hash da geração calculado = `generation.hash` | `J0955` | sim |
| V22 | mundo WIT de cada app aceite pela runtime alvo (`org.joker.worlds`; M21; a causa é o `J-E0001` da ref/04) e, num nó `prod`, nenhuma app com funcionalidades instáveis (chave 10 do `joker:meta`; causa `J-E0003`) | `J0958` + causa | sim |
| V23 | MCUs: placa, ABI e contador do firmware; o blob cabe nas capacidades do firmware alvo (`J0710` como causa) | `J0963`, `J0964` | sim |
| V24 | pré-compilação AOT das apps para a runtime alvo (`.cwasm`) | `J0954` + causa | sim |

Um DSD que falha em V1–V8 **não existe** para o nó: não muda nada, gera um evento `security` e o alarme `joker.agent.refused`. Um que falha em V9–V24 fica `refused` (V9–V16, V18–V24) ou `failed` (V17), com o código e as causas no relatório; o nó continua exatamente como estava. O V10b é a exceção: uma entrada de segredo recusada não recusa o DSD (a geração só falha, no V20, se precisar dela).

O artefacto de configuração vem primeiro porque é ele que diz o `mode` (V19) e o `[security] trust` (V20), que decidem as assinaturas dos outros.

### 3.4 Aplicação

#### 3.4.1 Máquina de estados

```
             V1–V8                V9–V16          V17              V18–V24
 (recebido) ───────▶ accepted ──▶ validating ──▶ fetching ──▶ staging ──▶ waiting ──▶ applying ──▶ confirming ──▶ confirmed
     │                  │            │             │             │           │            ▲               │
     └─ recusa          └─ noop      └─ refused    └─ failed     └─ refused  │            └── fase seguinte ─┤
        (não existe)                                                        └─ superseded                  └─ rolled_back
```

`applying` e `confirming` repetem-se por fase, pela ordem **runtime → mcu → generation**. Cada fase confirmada fica confirmada: se a fase `generation` fizer rollback, a runtime nova e os MCUs já confirmados ficam; o estado final é `rolled_back`, com a fase no evento. Uma fase sem mudanças (alvo igual ao atual) é saltada.

**Porquê esta ordem.** A runtime nova arranca primeiro com a geração **atual** (a regra de ouro da 1.x garante que a lê), e só depois recebe a nova, que pode precisar dela (V20 e V22 validam contra a runtime alvo). Os MCUs vêm antes da geração do nó porque o lock do nó fixa o que cada MCU exporta no `HELLO` (documento 19, §6): a geração nova do nó pode depender do firmware novo, e nunca o contrário.

#### 3.4.2 Concorrência entre DSDs

- Um DSD novo aceite substitui (`superseded`) o anterior se este ainda não começou a trocar (`accepted` a `waiting`). Os artefactos já descarregados ficam na cache por digest.
- Se o anterior está em `applying` ou `confirming`, o novo vai até `staging` e espera (`wait = confirmation`) que a fase em curso termine. Nesse momento, o que falta do anterior é abandonado (`superseded`) e o plano do novo é recalculado contra o estado real.
- **Um rollback nunca espera:** não conta para `max_applies_per_day`, ignora janelas, pausas e `hold`.
- **O mesmo trinco da API.** O agente e a API local (ref/09 §5.13) partilham o trinco de transição de gerações: no máximo uma geração `pending` e uma `confirming` por nó, venha de onde vier. Uma troca do agente espera (`wait = confirmation`) por uma transição local em curso; um `POST /v1/generations` durante uma troca do agente recebe `J-E1608`.

#### 3.4.3 Fases

| Fase | Troca | Janela de saúde | Rollback |
|---|---|---|---|
| `runtime` | binário no slot inativo; o `jokerd` reinicia no novo slot **com a geração confirmada atual**; os equipamentos cobrem o intervalo com o fallback (M1) e os MCUs com o seu lease | `health_window` + `confirm` | reinício no slot anterior; automático também se o `jokerd` novo não arrancar (systemd, documento 01, §16) |
| `mcu` | §3.6 | `mcu-target.confirm` | o MCU reverte para a imagem anterior |
| `generation` | o pipeline do documento 01, §15: planear o diff, preparar, trocar num limite de tick de cada grupo afetado | `confirm` | troca de volta para a geração anterior, mantida quente (`keep_previous`) |

Numeração das gerações: o `gen` inteiro é atribuído pelo nó quando a geração entra em staging. Um rollback **reativa** a geração anterior com o mesmo hash e **o mesmo número** (D9); a que falhou fica no histórico com o seu, em `rolled_back`. O histórico é de **ativações**, cada uma com a sua `origin` (`gen-origin`, §2.2: `agent` com o `seq` do DSD, `rollback`, …), o que a API mostra em `GET /v1/generations` (ref/09 §5.13).

#### 3.4.4 Espera antes de trocar

Uma fase só troca quando todas as condições se verificam; senão fica em `waiting` com o motivo (`J0959`):

| Motivo | Condição para avançar |
|---|---|
| `not_before` | agora ≥ `schedule.not_before` |
| `window` | agora está dentro de uma janela do DSD **e** de uma janela local (`[agent] maintenance`); sem janelas de um dos lados, esse lado não restringe. Uma interseção vazia nos próximos 7 dias é reportada logo |
| `jitter` | passou o atraso sorteado (contado da primeira vez que as outras condições se verificaram) |
| `clock` | há `windows` ou `not_before` e o relógio está `unsynced` (com `holdover` avança) |
| `paused` | `jk agent pause` ativo |
| `local_hold` | §3.7.1 |
| `confirmation` | outra fase em confirmação |
| `rate_limit` | `max_applies_per_day` atingido (`J0962`) |
| `mcu_link` | o MCU da fase não está `up` |

O download e o staging não esperam por nada disto: fazem-se logo, para que a troca, quando a janela abre, demore segundos.

### 3.5 Confirmação e saúde

A janela de confirmação começa na troca. As verificações de saúde:

| # | Verificação | Se falhar |
|---|---|---|
| H1 | nenhum grupo de ciclo em `overrun`, `tripped` ou `stopped` | reinicia o `stable_for` |
| H2 | nenhuma app em `crashloop` ou `failed` | **rollback imediato** (`J0961`) |
| H3 | nenhum trip novo desde a troca | **rollback imediato** (`J0961`) |
| H4 | cada dispositivo que estava `up` antes da troca está `up` ou `degraded`; cada dispositivo novo chega a `up` ou `degraded` | reinicia o `stable_for` |
| H5 | nenhum alarme novo ativo com gravidade ≥ `alarm_severity` | reinicia o `stable_for` |
| H6 | só na fase `runtime`: o primeiro tick de cada grupo dentro de `health_window` e os drivers ligados (documento 01, §16) | rollback no fim de `health_window` |
| H7 | `require_uplink`: depois da troca, pelo menos uma troca autenticada completa com o Deck (um `report-ack`) | impede a confirmação `auto` |

| `mode` | Confirma quando | Rollback quando |
|---|---|---|
| `auto` | H1–H6 seguidas durante `stable_for` **e** H7 | H2, H3 (imediato); fim da janela sem confirmar (`J0960`, ou `J0961` se a saúde estava a falhar) |
| `deck` | chega um comando `confirm` com o (`seq`, fase, hash) certo; o nó exige H1–H6 no momento, senão responde `conflict` | H2, H3; `reject` (`J0969`); fim da janela (`J0960`) |
| `manual` | `jk confirm` local ou numa sessão remota | H2, H3; `jk rollback`; fim da janela (`J0960`) |

O H7 é o que faz do `auto` uma confirmação honesta no sentido da ADR 0015: uma geração que cortou a rede ao nó não se consegue confirmar a si própria.

### 3.6 MCUs atrás do nó

Para cada `mcu-target`, por `order`, **um de cada vez**:

1. O MCU tem de estar `up` no Joker Link (senão `wait = mcu_link`; ao fim de `fetch_deadline`, `J0965`).
2. O nó já verificou a assinatura (papel `firmware` ou `config`); o MCU verifica-a **outra vez** com as suas próprias chaves (o bootloader e o `joker-micro` para o firmware, o `jk_generation_open` para o blob; ref/08 §2.12.4, §2.13.5). O MCU é a autoridade final sobre o que arranca.
3. **Firmware:** o nó envia a imagem pela mensagem `OTA` (ref/07); o MCU grava-a no slot inativo e reinicia em modo de teste (MCUboot *test swap* ou equivalente). Durante o reinício (milissegundos), as saídas do MCU estão no `safe` do próprio MCU.
4. O nó espera o `HELLO` com o digest novo (chave 5.2; e 5.9, o do descritor) e o firmware em experiência (12.4) dentro de `confirm.window`, e depois `stable_for` com o link `up`, a geração do MCU a correr e sem trips novos no MCU. Então confirma pela `OTA` (o MCU marca a imagem como permanente). Se não, o MCU reverte no reinício seguinte, que o nó provoca (`J0965`).
5. **Blob de geração:** o mesmo caminho, com o blob `joker.generation.v1` (ref/08 §2.13), trocado a quente (`jk_generation_swap`) com a janela da `OTA` `activate` (`confirm_ms`) e o anti-rollback por `gen_seq` (ref/07 §9.4–§9.5). O `HELLO` seguinte mostra o `gen_seq`, o hash e o estado de confirmação (chaves 6 e 12.7).
6. Uma falha pára a fase: os MCUs seguintes ficam por atualizar e os anteriores ficam confirmados.

O estado de cada MCU aparece em `report-body.mcus`, e cada passo gera um evento `apply` com `phase = mcu` e `device`.

### 3.7 Alterações locais, pausa e importação

#### 3.7.1 Alterações locais (local-first)

O `jk apply`, o `jk rollback` e o `jk runtime upgrade` locais funcionam sempre, com o papel `admin`. Quando uma pessoa muda a geração ou a runtime localmente (`origin = local` ou `session`):

| `[agent] local_changes` | Efeito |
|---|---|
| `hold` (omissão) | o nó continua a aceitar, descarregar e preparar DSDs, mas **não troca** (`wait = local_hold`, `J0966`, alarme `joker.agent.hold`). A espera termina com `jk agent resume`, com um DSD com `override_local = true`, ou com um DSD cuja geração e runtime são iguais às locais (o Git apanhou a mudança) |
| `overwrite` | o próximo DSD aplica-se normalmente |

A omissão protege o técnico que corrigiu uma coisa no local às 3 da manhã de ver a correção desfeita pelo Deck às 3h05. O relatório mostra a deriva (`drift`), e o Deck decide.

#### 3.7.2 Pausa

`jk agent pause --ttl <d> --reason "…"` suspende as trocas (não os downloads, os relatórios, os eventos nem os rollbacks) até `ttl` (≤ 7 d) ou `jk agent resume`. Evento `agent` `paused`/`resumed`. Serve para o comissionamento: o Deck não troca nada enquanto alguém está a trabalhar no quadro.

#### 3.7.3 Importação (sites sem rede)

`jk agent import <dir>` aplica um DSD trazido à mão. O diretório tem `desired.cose` e um *OCI image layout* (`oci-layout`, `index.json`, `blobs/sha256/…`) com os artefactos. O processamento é o do §3.3 e §3.4, com o diretório como primeira fonte e `origin = import`. As regras de `seq` aplicam-se: um DSD importado e depois recebido pela rede não se aplica duas vezes. Como não há Deck, o H7 não se pode cumprir: numa importação, `mode = auto` com `require_uplink = true` passa a `manual`, e a pessoa confirma com `jk confirm`.

### 3.8 Estado reportado

#### 3.8.1 Completo e delta

| Envia-se um relatório completo | Envia-se um delta |
|---|---|
| depois do `hello` com `need_full = true` ou com `boot` novo | em cada mudança, agregada durante `report_min_interval` |
| depois de cada transição de fase de uma aplicação | a cada `heartbeat` se nada mudou (só `resources` e `agent`) |
| a cada `full_interval`, com um desvio de `hash(nó) mod full_interval` para espalhar a frota | — |
| com o comando `report_full` | — |
| quando um delta excederia 32 KiB | — |

**Deltas.** O nó guarda o último relatório **confirmado** (`report-ack`). Um delta é o merge patch (RFC 7386) desse relatório para o estado atual, com `base` = o seu `seq`. Se o ack se perder, o delta seguinte continua a ter a mesma `base` e inclui as mudanças anteriores: um delta repetido ou perdido nunca deixa o Deck num estado errado. O Deck aplica o delta se o `seq` que tem guardado for a `base`; senão responde `need_full`.

#### 3.8.2 Frequência e custo

| Cenário | Completo | Delta típico | Volume mensal (agente, sem telemetria) |
|---|---|---|---|
| bomba de calor (documento 08, rodada 5), JSON em HTTPS | ~3 KiB | ~200 B | ~2 MB (delta a cada 5 min, completo a cada 6 h) |
| idem, CBOR em MQTT, `heartbeat = 30m` | ~1,2 KiB | ~90 B | ~0,5 MB |
| BESS com 50 dispositivos e 200 instâncias, JSON | ~25 KiB | ~1 KiB | ~15 MB |
| Joker Micro, CBOR | ≤ 1 KiB | ~60 B | ~0,2 MB |

Os bytes do agente contam para o orçamento do uplink da mesma ligação (M12, documento 01, §13): eventos `alarm`, `security` e `apply` na classe `alarms`; relatórios e outros eventos na classe `events`; artefactos e uploads na classe `bulk`. Sob pressão de orçamento, o nó alarga `report_min_interval` até 15 min e `full_interval` até 24 h, e nunca atrasa as classes `alarms`.

#### 3.8.3 Truncagem

Um completo acima de 256 KiB é cortado, por esta ordem, até caber: `devices` em `up`, `instances` em `ok`, `apps` em `running`, `groups` em `ok`, `alarms` de gravidade `low`. As chaves cortadas vão em `truncated`, e o nó regista `J0971`. O que está mal nunca é cortado antes do que está bem.

### 3.9 Eventos

#### 3.9.1 Produção

Cada evento recebe o `seq` quando é escrito no registo local (joker-store). Os de classe A são escritos com `fsync = "always"` (como os trips e as entradas críticas do registo de decisões, documento 01, §12 e §19); os de classe B e C em lote (1 s). Numa falta de energia perde-se no máximo 1 s de eventos B e C, que nunca chegaram a ter `seq` persistido e por isso não deixam buraco.

#### 3.9.2 Entrega

- **Pelo menos uma vez**, por ordem de `seq`, em lotes de até `event_batch`.
- O Deck responde com o **ack cumulativo**: o maior `seq` S tal que todos os eventos ≤ S estão guardados de forma durável no Deck ou declarados perdidos por um `gap`. O nó apaga da fila tudo o que é ≤ S.
- Sem ack em 30 s, ou depois de uma reconexão, o nó reenvia a partir de S + 1. O Deck deduplica por (nó, `seq`); o `id` (ULID) deduplica entre sistemas (por exemplo, depois de exportar para outro sistema).
- Em MQTT, o PUBACK do broker **não é** o ack: só o `ack/events` assinado pelo Deck apaga eventos. Um ack antigo repetido só confirma o que já estava confirmado; um ack acima do `event_seq` do nó é ignorado.
- Depois de uma reconexão, o completo vai **antes** da fila acumulada: o Deck sabe o estado atual antes de digerir o passado.

#### 3.9.3 Fila e o que se perde

A fila é um cursor sobre o registo local, com o teto `queue.max_disk`. Quando está cheia, o nó descarta, por esta ordem: o evento C mais antigo; o B mais antigo; o A mais antigo. Cada descarte:

- mantém o `seq` do evento (o Deck vê o buraco);
- entra num evento `gap` (nunca descartado; os `gap` seguidos fundem-se, até 64 intervalos) com os intervalos e as contagens por tipo;
- incrementa `joker.agent.dropped` e levanta `joker.agent.queue_overflow` (`J0970`).

**O que se perde é o envio, e não o registo.** As decisões, os alarmes e os trips continuam no registo local, sujeitos à retenção local (documento 01, §12), e o Deck pode pedi-los com o comando `upload` depois de ver o `gap`. Com 64 MiB e ~200 B por evento, a fila guarda ~300 000 eventos: meses de um site normal offline.

No perfil micro, a fila é um anel de 32 eventos em RAM (mais flash, se o firmware lhe der espaço), com a mesma ordem de descarte e o mesmo `gap`.

#### 3.9.4 Esvaziar a fila

Depois de muito tempo offline, o nó envia a fila a `drain_rate` eventos/s (omissão 50/s), dentro do orçamento do uplink. Um Deck que precisa de respirar responde `rate_limited`.

### 3.10 Sessão remota (agent/1.3)

**Abertura.**

1. Uma pessoa pede a sessão no Deck. A autenticação da pessoa (e o MFA) é do Deck.
2. O Deck envia o comando `session_open`, assinado.
3. O nó verifica, por ordem: suporta a 1.3; `[agent.remote] allow` (`J0980`); o transporte tem HTTPS (`J0985`); `max_sessions` (`J0982`); `ttl` ≤ `max_ttl` (`J0983`); o **papel** (abaixo, `J0981`).
4. Se `consent = "local"`: levanta `joker.agent.session_request` e espera `jk session accept <id>` (papel `operator`, no socket local) ou o `consent_signal` a verdadeiro, até `consent_timeout` (`J0984`).
5. O nó abre o túnel (`wss://`, mTLS) para o URL do comando, que tem de estar num dos `deck_urls`.
6. Evento `session` `opened`; alarme `joker.agent.session`.

**O papel é do nó.** O papel efetivo é o menor de três: o pedido, `max_role`, e o que o `[security.roles]` do manifesto dá ao principal. Os principais das sessões escrevem-se `deck:<deck_id>/user:<id>` e `deck:<deck_id>/group:<grupo>` (por exemplo `operator = ["deck:deck-prod/group:ops-porto"]`). Sem nenhuma entrada que case, vale `default_role`; vazio, recusa. Um pedido acima do permitido é recusado (`J0981`), e não reduzido em silêncio: quem pediu sabe o que tem.

**Onde termina.** Cada fluxo do túnel termina na API local, que aplica os papéis, o envelope, o árbitro e o TTL das escritas manuais (A10), como para qualquer cliente. O túnel não leva a mais nada: nem shell, nem reencaminhamento de portas para os equipamentos de campo, nem a outro nó.

**Proibido em sessões, com qualquer papel** (`J0986`): os endpoints de identidade e confiança (`/v1/node/*`), as mudanças ao próprio agente (`/v1/agent/*` exceto as leituras: `pause`, `resume`, `import`, aceitar sessões) e os segredos (`/v1/secrets`). O Deck não consegue usar uma sessão para alargar a trela com que o nó o segura. A lista é a da ref/09 §2.3.

**Fim.** Por `ttl`, por inatividade (`idle`), por `session_close`, por `jk session close` local, por reinício do `jokerd` (as sessões não sobrevivem), ou por revogação do certificado. Evento `session` `closed`/`expired` com as contagens de pedidos e escritas.

**Auditoria.** Cada pedido no túnel entra no registo de auditoria local da API (ref/09 §4) com o `id` da sessão e o principal `deck:<deck_id>/user:<id>`; `GET /v1/audit?session=<id>` mostra-os. Cada escrita entra no registo de decisões com `writer = user:deck:<deck_id>/user:<id>` e `session`, e é sempre enviada ao Deck (§2.11).

**Limites.** `max_rate` por sessão (balde de tokens), 16 fluxos, crédito inicial de 256 KiB por fluxo, `ping` a cada 30 s.

### 3.11 Transportes

#### 3.11.1 HTTPS e long-poll

```
nó                                           Deck
 │── TLS 1.3 (mTLS; pinos deck_tls) ────────▶│
 │── POST /hello ───────────────────────────▶│
 │◀─ hello-reply {need_full, event_ack} ─────│
 │── POST /report (completo, se pedido) ────▶│
 │── POST /events (fila, por ordem) ────────▶│
 │── GET /poll?desired=418&cursor=c&wait=55s▶│  o Deck segura o pedido
 │◀─ poll-reply {desired?, commands?} ───────│  logo que haja DSD com seq > 418, comandos, ou ao fim de wait
 │── GET /poll … (de imediato) ─────────────▶│
```

- Relatórios e eventos vão em pedidos à parte na **mesma** ligação HTTP/2.
- O poll é também o sinal de vida do nó para o Deck.
- Com `poll_interval` > 0 (LTE), o nó faz polls curtos (`wait = 0`) a esse intervalo, e envia deltas e eventos quando os tem. Um alarme `critical` provoca um poll imediato.

#### 3.11.2 MQTT

1. `CONNECT` com `clean start = false`, a *will* `status` assinada (`online = false`) e o certificado.
2. `SUBSCRIBE` aos tópicos Deck → nó. O DSD retido chega logo.
3. `PUBLISH` do `hello`, depois do `status` (`online = true`).
4. O Deck responde em `hello/reply`, e o resto segue como em HTTPS, com os acks nos tópicos `ack/*`.

**Tudo é assinado de ponta a ponta** (§2.14): o broker é transporte, não é confiança. Um broker comprometido consegue atrasar ou deitar fora mensagens, e não consegue forjar um relatório, um evento, um ack nem um DSD. A repetição de mensagens antigas é inócua: `seq` nos relatórios e eventos, e acks que só confirmam o que existe.

#### 3.11.3 Artefactos

O nó descarrega por digest com a API de distribuição OCI (`GET /v2/<nome>/blobs/<digest>`, com `Range` para retomar), com o token do `POST /registry-token` quando `auth = token`. As assinaturas procuram-se pela API *referrers* ou pelos digests em `artifact.signatures`. Uma cache local por digest evita descargas repetidas; um blob parcial guarda-se 7 d para ser retomado por um DSD seguinte. Cada fonte é tentada por ordem (espelhos locais, depois as do DSD), e cada artefacto é retentado com backoff de 30 s a 1 h até `fetch_deadline`.

#### 3.11.4 Reconexão e backoff

- Atraso da tentativa *n*: aleatório uniforme em [0, min(`backoff_max`, 1 s × 2ⁿ)] (*full jitter*). Volta a *n* = 0 depois de uma ligação que durou mais de 60 s.
- `Retry-After`, `retry_after` e `poll-reply.backoff` são mínimos: o nó espera o maior entre esse e o seu.
- Três falhas seguidas num URL passam ao seguinte de `deck_urls` (ou de `mqtt`); depois do último, volta ao primeiro.
- Um erro de TLS por pino (`J0975`) não é repetido mais do que uma vez por hora: é quase sempre configuração, e não rede.

#### 3.11.5 Meses offline

| O que acontece | Como se resolve |
|---|---|
| o nó controla no último estado confirmado | por desenho (princípio 1) |
| um DSD em `waiting` continua à espera | aplica-se quando a janela abre, se ainda for o último |
| o certificado expira | `POST /renew` com o certificado expirado, até `renew_grace` (§3.2.1) |
| o bundle ficou para trás (chaves `deck` rodadas) | `hello-reply.trust` traz a cadeia; o nó verifica versão a versão antes de verificar o DSD |
| a fila de eventos enche | descarte com `gap` (§3.9.3); o registo local fica |
| o relógio derivou | §3.12 |
| a runtime do nó já saiu do suporte do Deck | núcleo mínimo (§3.14) |

O Deck **não guarda uma fila de DSDs**: guarda o último. O nó que volta recebe o estado desejado de hoje, e não as 26 versões semanais por que passou. É o que torna o regresso de 6 meses uma aplicação só.

### 3.12 Relógios

1. **O nó nunca recusa nada do Deck por tempo com o relógio não `synced`.** A frescura vem do `seq` (DSD) e da ligação ao alvo (comandos). Com o relógio `synced`, aplicam-se `expires`, `issued` e a janela de ±30 s dos comandos (ref/00, §9).
2. **TLS em 1970.** Com o relógio `unsynced`, o nó ignora as datas de validade do certificado do Deck e aceita-o só se o SPKI estiver em `deck_tls`. Sem pinos, o nó espera pelo relógio para falar com o Deck (e isso fica no `jk doctor`). Por isso os bundles de produção devem ter pinos.
3. **Tempo do Deck.** Cada `hello-reply` e `poll-reply` traz `time`. O nó calcula o desvio (com metade do RTT) e mostra-o no `jk agent status`. O nó usa-o como fonte grosseira do relógio (ref/02), com qualidade no máximo `holdover`, nunca `synced`.
4. **Do lado do Deck**, um relatório ou evento nunca é recusado por desvio. O Deck calcula-o, marca-o acima de 5 min (`J0972`) e corrige os tempos dos eventos com o par (`boot`, `mono`) e os eventos `clock`, que o nó emite quando o relógio muda de qualidade ou salta (é a correspondência monotónico ↔ UTC do documento 01, §18, enviada para fora).
5. As janelas de manutenção exigem `synced` ou `holdover`; com `unsynced`, a fase espera (`wait = clock`).

### 3.13 Perfil micro: comportamento

- Mesmas regras de validação (V1–V24, com o subconjunto do §2.15) e a mesma máquina de estados, com as fases `runtime` (o firmware) e `generation` (o blob).
- **Firmware:** gravado no slot inativo; reinício em modo de teste; confirmado pelo próprio MCU depois de `stable_for` com H1–H5 e, se `require_uplink`, de um `report-ack` do Deck. Sem confirmação, o bootloader reverte no reinício seguinte (o watchdog garante que ele acontece).
- **Blob de geração:** gravado na área de geração inativa e trocado a quente num limite de tick (`jk_generation_swap`, ref/08 §2.12.4), com a mesma confirmação; sem ela, o MCU volta à geração anterior sozinho. Os trips persistem em flash entre reinícios.
- O MCU guarda em flash: o bundle, os pisos, o último `seq` aceite, o `seq` dos relatórios e dos eventos, e o estado da aplicação em curso.
- Relatórios: completo no arranque e a cada `full_interval`; deltas agregados durante 30 s no mínimo.
- Sem sessões remotas, sem `upload`, sem MCUs atrás.

### 3.14 Versões e compatibilidade

**Negociação.** O nó envia `proto = [1, N]` no `hello`; o Deck responde com `proto_minor = min(N, M)`. As mensagens e chaves de uma minor acima da negociada não se enviam.

**Núcleo mínimo.** `hello`, `poll` e `desired`, `report`, `events`, `renew`, e os comandos `confirm`, `reject` e `report_full`, na forma da `agent/1.0`, **nunca mudam na série 1** (ADR 0022, decisão 1: o protocolo do agente não quebra numa 1.x). Todo o Deck da série 1 aceita o núcleo mínimo de qualquer nó 1.x.

**Suporte.** Um Deck suporta por inteiro nós das **três últimas minors e da LTS** (ADR 0022, decisão 5): todas as funcionalidades, testadas. Um nó mais antigo recebe só o núcleo mínimo (`hello-reply.code = J0973`, aviso no Deck), que chega para lhe mandar um DSD com uma runtime nova. Um nó nunca fica sem caminho para ser atualizado.

**O que uma minor pode acrescentar** (ref/00, §1): chaves opcionais, variantes de enum no fim, comandos, tipos de evento, endpoints e tópicos. Uma chave nova no DSD que um nó antigo **não pode** ignorar vai em `crit`; o nó antigo recusa o DSD (`J0946`) e o Deck sabe, pelo `proto_minor`, que não a deve enviar.

| Minor | Acrescenta |
|---|---|
| `agent/1.0` | tudo o que este documento não marca com outra versão |
| `agent/1.1` | tipo `joker.model.v1+jkm` e papel `models`; chave 23 do DSD; `report-body` 20 (modelos, sombra) |
| `agent/1.3` | sessões remotas (comandos 5 e 6, túnel, eventos `session`); `report-body` 19 (intenções pendentes); atestação TPM exigível no enrolamento |
| `agent/1.5` | nada de obrigatório; o Deck agrega auditoria a partir de `upload` e dos eventos `decision` |

**v2.** O protocolo da v2 descreve **sites** (vários nós e MCUs como um sistema; documento 21, §8). Fica reservado, sem semântica:

- o identificador `agent/2`, o prefixo HTTPS `/agent/v2`, o prefixo MQTT `jk/2/` e o recurso CoAP `/a2/`;
- as chaves 100–119 do `desired-state` e do `report-body` (em particular: 100 `site`, 101 `members`, 102 `placement`, 103 `site_generation`);
- o primeiro segmento `@` nos caminhos (ref/00, §2), para caminhos qualificados por nó em relatórios e eventos.

Um Deck da série 2 fala `agent/1` com os nós 1.x durante toda a série (documento 21, §1.1).

---

## 4. Estresse

**S1 — O Deck é comprometido.** O atacante tem a base de dados, a CA emissora e a chave `deck`.

| Consegue | Não consegue |
|---|---|
| deixar de entregar atualizações (congelar a frota) | fazer um nó correr um artefacto que não esteja assinado por uma chave de pipeline do bundle (princípio 5) |
| emitir DSDs com artefactos **antigos mas válidos** | ir abaixo de um piso, nem a uma runtime mais antiga sem `allow_downgrade`, nem a um digest revogado (V15) |
| fazer trocas a más horas, dentro das janelas **locais** do nó | trocar fora de `[agent] maintenance`, nem mais de `max_applies_per_day` vezes |
| confirmar ou rejeitar gerações em confirmação | impedir o rollback automático de uma geração que falha a saúde |
| abrir sessões com utilizadores inventados, com o papel que o manifesto dá aos principais `deck:` | ter mais do que esse papel; mexer na identidade, na confiança ou no agente; escrever sem TTL ou fora do envelope |
| ler relatórios, eventos e uploads (perda de confidencialidade) | ler segredos (nunca passam pelo protocolo) |
| emitir certificados para nomes de nós | fazer-se passar pelo Deck perante os nós sem o SPKI dos pinos; mudar a `authority` |
| mudar o `[security] trust` do manifesto | — o manifesto é assinado pela chave `config`, que não está no Deck |

**Correções incorporadas:** a janela local de manutenção e o limite de trocas por dia (sem eles, um Deck comprometido podia reiniciar 3 000 `jokerd` a cada minuto, alternando entre duas runtimes válidas, e pôr todos os equipamentos em fallback); a proibição de mudar o agente e a confiança dentro de sessões; o `[security] trust` do manifesto que só restringe. **Resíduo:** o congelamento e a escolha de versões antigas acima do piso. Mitigação operacional: pisos publicados pelo pipeline a cada release de segurança, e alarme no pipeline se um nó não reportar a versão nova em N dias. **Aguenta.**

**S2 — Rollout de 3 000 nós** (documento 08, rodada 5). O Deck emite DSDs por ondas (1% → 10% → 100%), com `windows` das 2h às 5h e `jitter = "45m"`. Os nós descarregam logo (2,8 MB de app × 3 000 = 8,4 GB, servidos pelo registo ou por uma CDN, não pelo Deck), e trocam dentro da janela, espalhados em 45 min. Carga no Deck: 3 000 long-polls abertos (~55 pedidos/s em regime), relatórios completos depois de cada fase (~9 000 em 3 h, ~1/s). A primeira onda confirma em `auto` com `stable_for = "5m"`; o Deck vê os eventos `apply` e os KPIs (documento 20, D2), e, se a regra de paragem disparar, **simplesmente não emite** os DSDs das ondas seguintes, e emite para a primeira onda um DSD com a geração anterior (`seq` + 1). Os nós em confirmação recebem `reject`. Os 300 gateways em LTE (50 MB/mês) recebem o mesmo DSD, mas a descarga de 2,8 MB vai na classe `bulk` com `fetch_deadline = "7d"`: gasta 6% do orçamento do mês e nunca atrasa um alarme. **Achado:** sem `fetch_deadline` longo e classe `bulk`, o OTA mensal comia o orçamento dos alarmes. **Corrigido** (§3.8.2). **Aguenta**, e a porta da 1.5 (99,9% no estado desejado em 24 h) depende das janelas, e não do protocolo.

**S3 — Nó offline 6 meses.** Volta com a runtime 1.0.3 e o Deck já está a instalar a 1.2. O certificado (365 d) ainda vale; se não valesse, `POST /renew` com o expirado. O `hello` diz `trust_version = 3`; o `hello-reply` traz as versões 4 e 5 (uma rotação da chave `deck`), que o nó verifica pela ordem. O Deck entrega **o DSD de hoje** (runtime 1.2 + geração de hoje), e não os 26 intermédios. O nó aplica a fase `runtime` (a 1.2 lê a geração da 1.0, pela regra de ouro), confirma, e depois a `generation`. A fila tinha 64 MiB de eventos; o completo vai primeiro, a fila a seguir a 50/s (~100 min para 300 000 eventos), com um `gap` se encheu. Os eventos com relógio `holdover` são corrigidos pelo Deck com os `clock`. **Aguenta.**

**S4 — Dois Decks a disputar o mesmo nó.** (a) Um Deck de um integrador antigo continua a emitir DSDs para um nó que mudou de Deck: a chave dele já não é a `authority` (V3, `J0941`), evento `security` e alarme. (b) Um Deck em alta disponibilidade entra em *split-brain* e as duas metades emitem `seq = 512` com conteúdos diferentes: o nó aceita o primeiro que chega e recusa o segundo (`J0943`), e o evento diz ao Deck que tem dois emissores. (c) As duas metades emitem 512 e 513: o nó aplica o 513, porque o `seq` é a ordem, e o `superseded` do 512 aparece no relatório. **Achado:** sem `J0943`, o nó aplicava em silêncio o DSD de quem ganhasse a corrida. **Correção:** a regra V7 e a recomendação ao Deck de derivar o `seq` de um contador único (uma sequência na base de dados, nunca por instância). **Aguenta.**

**S5 — Estado desejado com um artefacto que o nó não consegue descarregar.** O registo apagou a tag, a CDN devolve 403, ou o artefacto nunca foi publicado. O nó tenta os espelhos e as fontes por ordem, com backoff por artefacto, e reporta `desired.state = fetching` com `fetched = [1,2 MB, 4,0 MB]`. Nada é trocado: o staging só começa com tudo descarregado e verificado, e a aplicação é tudo ou nada por fase. Ao fim de `fetch_deadline`, `failed` com `J0952` e o digest em `causes`. O nó continua na geração confirmada; o Deck corrige a publicação e emite `seq` + 1 (os blobs já descarregados estão na cache). Variante: o artefacto descarrega, mas o digest não confere (`J0950`) ou a assinatura não é de uma chave com o papel (`J0951`): `refused` na hora, sem esperar pelo prazo, e alarme de segurança. **Aguenta.**

**S6 — A geração nova corta a rede ao nó.** Uma rota errada no manifesto. A troca acontece, as saúdes locais H1–H5 passam, mas o H7 nunca se cumpre: não há `report-ack`. Ao fim de `window`, rollback (`J0960`), a rede volta, e o evento `apply` `rolled_back` chega ao Deck. **Aguenta**, e é exatamente a ADR 0015. **Achado:** a primeira versão do `auto` só olhava para a saúde local, e confirmava uma geração que tinha isolado o nó. **Corrigido** com o H7 e `require_uplink = true` por omissão.

**S7 — Arranque em 1970** (documento 08, rodada 5 (d)). O gateway armv7 sem RTC arranca com o relógio em 1970 e LTE fraco. O TLS com o Deck funciona pelos pinos (§3.12, 2); o DSD é aceite pelo `seq`; o `expires` é ignorado; a fase com `windows` espera (`wait = clock`); os eventos saem com `clock = unsynced` e são corrigidos depois pelo evento `clock`. **Achado:** sem pinos no bundle, o nó não fala com o Deck enquanto não tiver hora, e sem Deck não recebe a hora grosseira; ficava preso até ao NTP. **Correção:** os pinos `deck_tls` e o tempo do Deck como fonte grosseira (ref/02, §3.2.4). **Aguenta.**

**S8 — Cartão SD clonado.** Um técnico copia o cartão de um RevPi que funciona para montar um segundo. A chave estava em ficheiro. O clone liga-se com a mesma identidade e outra impressão de hardware: quarentena (`J0937`). Se os dois estiverem ligados ao mesmo tempo, os dois ficam em quarentena (`J0938`). Nenhum dos dois pára de controlar. A solução é `jk node enroll --force` no clone, que lhe dá uma identidade própria. Com TPM, o clone nem faz TLS. **Aguenta**, e o `J0933` em cada nó com chave em ficheiro torna o risco visível.

**S9 — 50 MB por mês em LTE.** Long-poll contínuo em HTTPS: um poll a cada 55 s com TLS retomado custa ~1–2 KB (as NAT de LTE fecham ligações paradas), 47 000 por mês, até ~90 MB. **Quebrou.** **Correção:** `poll = "15m"` (2 900 polls, ~5 MB) com poll imediato em alarmes críticos, ou MQTT com `keepalive = "20m"` e CBOR (~1 MB por mês incluindo relatórios). O `jk check` avisa se um nó com `[uplink] budget` usa long-poll contínuo. **Aguenta com a correção.**

**S10 — O Deck é reposto de uma cópia de segurança de ontem.** O contador de `seq` do Deck para o nó está em 510, e o nó já aceitou 514. Sem mais nada, todos os DSDs novos seriam `J0942` para sempre. **Correção incorporada:** o `hello` leva `desired_seq`, e o Deck passa a emitir a partir de max(o seu, o do nó) + 1. O mesmo para os eventos: o `event_ack` do Deck fica abaixo do que o nó já apagou; o nó reenvia o que ainda tem, e o resto aparece como buraco sem `gap`, que o Deck recupera com `upload` (`what = events`). **Aguenta.**

**S11 — Nó 1.0 e Deck 1.5.** A 1.0 já saiu do suporte (12 meses). O Deck fala com ele pelo núcleo mínimo (`J0973` como aviso), não lhe envia `crit` que ele não conheça, e entrega-lhe um DSD com a runtime 1.5. O nó recusaria um DSD com um modelo (`J0957`), e o Deck sabe-o pelo `proto_minor = 0`. **Aguenta.**

**S12 — Um técnico faz `jk apply` no local a meio de um rollout.** Às 3h00 corrige um endereço Modbus; às 3h05 o Deck entrega a onda 2. Com `local_changes = "hold"`, o DSD é preparado, mas não trocado (`J0966`); o relatório mostra `origin = local` e a deriva. Quando o endereço corrigido chega ao Git e o Deck emite um DSD com essa geração, o hash coincide e a espera termina sozinha. **Achado:** sem `hold`, a correção era desfeita em 5 minutos, e o técnico voltava a corrigir, e o Deck a desfazer. **Aguenta.**

---

## 5. Decisões e alternativas rejeitadas

**D1 — Estado desejado completo e idempotente, com `seq`.** Rejeitada: uma fila de ordens ("instala X", "reinicia Y"), como em vários gestores de dispositivos. Uma fila offline durante meses tem de ser reproduzida por ordem, e uma ordem perdida ou repetida deixa o nó num estado que ninguém escolheu. O estado completo dá o regresso de 6 meses numa aplicação só (S3).

**D2 — Duas camadas de assinatura.** O DSD é assinado pela chave `deck` (que vive no Deck), e os artefactos pelas chaves do pipeline (que não vivem). Rejeitado: assinar o DSD no pipeline (as ondas, as janelas e os rollbacks passavam a precisar de uma cerimónia de assinatura); e não o assinar de todo (o TLS não protege quando há um broker MQTT no meio nem numa importação em pen USB).

**D3 — Documentos assinados sempre em CBOR/COSE, também em HTTPS.** Rejeitado: JWS em JSON. Duas famílias de assinatura e o problema da canonicalização de JSON; a ref/00 já fixa COSE com CBOR determinístico.

**D4 — Long-poll e MQTT; nada de canal de controlo permanente.** Rejeitados: gRPC (HTTP/2 com Protobuf, sem `no_std` útil e sem par em MQTT), e um WebSocket permanente para controlo (proxies industriais cortam-no, e passava a ser o "caminho de controlo" que o princípio 2 proíbe). O WebSocket só existe para sessões, abertas pelo nó, com TTL.

**D5 — Deltas por merge patch (RFC 7386) sobre mapas chaveados por nome.** Rejeitado: JSON Patch (RFC 6902), com caminhos por índice e dependente da ordem. O merge patch funciona igual em CBOR e em JSON, e um delta perdido não estraga nada (a base é sempre o último confirmado).

**D6 — Ack cumulativo e aplicacional.** Rejeitados: ack por evento (conversa demais em LTE) e o QoS 2 do MQTT (só garante a entrega ao broker, e não ao Deck).

**D7 — Bundle de confiança com raiz fora de linha, pisos por papel: uma versão curta do TUF.** Rejeitados: TUF e Uptane completos. São a direção certa e o desenho é compatível com eles (raiz, papéis delegados, rotação encadeada, proteção contra downgrade), mas o volume de metadados e de assinaturas não cabe num MCU de 2 KiB de DSD na v1.

**D8 — Uma só autoridade por nó.** Rejeitado: vários Decks com direitos iguais (resolver conflitos entre eles no nó é inventar um protocolo de consenso). A mudança de autoridade é explícita e assinada pela raiz.

**D9 — Nenhuma escrita em sinais no protocolo.** As escritas remotas só existem em sessões, pela API local. Rejeitado: um comando `write` no agente, que seria um segundo caminho de escrita, com outra autenticação, outra auditoria e outra forma de errar. Uma escrita remota é igual a uma local (AGENTS.md, invariante 3).

**D10 — Identidade em P-256, renovação com chave nova, e renovação com certificado expirado.** Rejeitados: Ed25519 na identidade (os TPM, os elementos seguros e o mbedTLS não o fazem no TLS), e EST (RFC 7030), que obriga a PKCS#7 e a autenticação HTTP Basic no enrolamento; a semântica do `/enroll` e do `/renew` é a do EST, sem os formatos.

**D11 — Dois tipos de artefacto novos: `joker.runtime.v1` e `joker.config.v1+cbor`.** O documento 20, §3.2, não tinha como distribuir a runtime Linux nem a configuração de um nó Linux como artefacto; sem eles, o DSD não tinha para onde apontar. A alternativa (a runtime como pacote `.deb` e a configuração como commit Git a descarregar pelo nó) punha o Git e o APT no caminho de cada nó.

**D12 — Fases por ordem fixa (runtime → mcu → generation).** Rejeitado: ordem declarada no DSD. A ordem certa decorre das dependências (§3.4.1), e uma ordem configurável só servia para a escolher mal.

**D13 — CDDL com chaves em forma dupla `("nome" / n)`.** Rejeitados: dois esquemas separados (JSON Schema e CDDL), que divergem ao primeiro commit; e só chaves de texto também em CBOR (o dobro do tamanho num MCU).

**D14 — LwM2M para os MCUs, rejeitado.** Seria um segundo modelo de objetos ao lado das classes e dos sinais. O perfil micro é o mesmo protocolo, com menos chaves.

---

## 6. Marcações de versão

| Item | Versão |
|---|---|
| enrolamento, renovação, revogação, quarentena, bundle, pisos | 1.0 |
| estado desejado, comandos `confirm`, `reject`, `report_full`, `renew`, `upload` | 1.0 |
| estado reportado, eventos, fila com `gap` | 1.0 |
| HTTPS com long-poll, MQTT, CoAP (perfil micro) | 1.0 |
| perfil micro (MCUs autónomos); gestão em escala pelo Deck | 1.0; em escala, 1.5 |
| MCUs atrás do nó (fase `mcu`) | 1.0 |
| importação offline (`jk agent import`) | 1.0 |
| **Preparado para ML:** tipo `joker.model.v1+jkm`, papel `models`, chave 23 do DSD, `report-body` 20, capacidade `models` | reservado na 1.0; semântica na **1.1** |
| intenções pendentes no relatório (`report-body` 19) | reservado; 1.1 (M18) e aprovação no Deck na 1.3 |
| sessões remotas (comandos 5 e 6, túnel, eventos `session`, `[agent.remote]`) | **1.3** (definidas agora, recusadas com `unsupported` antes) |
| atestação TPM no enrolamento | opcional na 1.0; exigível pelo Deck a partir da 1.3 |
| segredos por destinatário (`joker.secrets.v1`, chave 22 do DSD, `report-body` 21) | 1.0 (D6, ADR 0024) |
| sites (chaves 100–119, `agent/2`, `jk/2/`, `/agent/v2`, `/a2/`) | reservado para a **v2** |
