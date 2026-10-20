# ref/09 — API HTTP local e WebSocket

Referência normativa da API local do `jokerd` (`joker-api`). O ficheiro máquina-legível é `spec/openapi/joker-api.yaml` (OpenAPI 3.1); este documento e esse ficheiro dizem o mesmo, e onde divergirem é um bug a corrigir nos dois (ADR 0023, ponto 3).

---

## 1. Âmbito e fontes

**Cobre:** transportes, autenticação e papéis, representação JSON, erros, paginação, filtros, idempotência, concorrência otimista, anti-replay, limites, todos os endpoints `/v1/…`, o protocolo WebSocket de subscrições e os Server-Sent Events, e o mapeamento de todos os comandos da `jk` (documento 07).

**A regra que define o âmbito:** a `jk` é só um cliente desta API (documento 01, §14; documento 07). Tudo o que a `jk` faz contra um nó passa por aqui, e portanto uma HMI, o Studio ou um script podem fazer o mesmo. Os comandos que não tocam num nó (`jk new`, `jk check`, `jk build`…) são locais e estão marcados na tabela do §9.

**Não cobre:** o protocolo nó ↔ Deck (ref/10), que chega ao mesmo núcleo por outro caminho; o Joker Link (ref/07); as bibliotecas cliente (ref/05, que geram código a partir do OpenAPI).

| Fonte | O que dá a esta referência |
|---|---|
| 01 §6, §6.1, §7 | árbitro, intenções, envelope: o que as escritas manuais, o `reset` e as intenções fazem |
| 01 §14–§19 | a API em si, gerações, OTA, segurança, tempo, observabilidade |
| 02 §5–§8, §9, §11 | amostras, leitura coerente, ciclo de vida de uma escrita, ações, caminhos e seletores, alarmes |
| 03 §13 | `[security] api` e `[security.roles]` |
| 07 | os comandos e os códigos de saída da `jk` |
| 16 | A1–A11, E1–E9, §3 qualidade, §7 traço, §9 códigos |
| 17 §4 | métodos e ações de classes |
| 19 §6, 20 §3 | MCUs por Joker Link, sessões remotas do Deck |
| 09 ataque 9, ataque 11 | modelo de ameaças da API, ~1 000 clientes WebSocket |
| 12 §5, §12 | fronteira da rede de campo; o Studio como cliente desta API |
| ADR 0013, 0015, 0022 | a API nunca trava o núcleo; confirmação; `/v1` e compatibilidade |

**Ligações a outras referências:**

| Referência | Relação |
|---|---|
| ref/00 | representação (tempo, valores, amostra JSON, qualidade, resultados, identificadores, erros, segurança, limites). Aplica-se aqui sem repetição |
| ref/01 | `security.api.listen`, `security.roles.*`, `max_manual`, `[alarms.*]`; a API expõe o manifesto efetivo e a proveniência de cada chave (§5.13) |
| ref/02 | configuração do nó (grupo do socket, CA de clientes e CRL, limites locais, quota de traços, `[policy] apply_from`), enrolamento e confiança (âncoras, bundle), segredos por destinatário (`secret-bundle`), configuração do cliente `jk` (`--node`), captura de factos para o lock, `[wcet]` |
| ref/04 | `propose` (intenções, 1.1), `invoke` (ações: os códigos da app e os da API, ref/04 §7.1), ids dos alarmes de apps (`app.<app>.<nome>`) |
| ref/05 | clientes Python e TypeScript gerados deste OpenAPI |
| ref/06 | manifesto de drivers da runtime (`GET /v1/runtime/drivers`), estado do link por dispositivo (`link_state`, ref/00 §6) |
| ref/07, ref/08 | estado dos MCUs (modo da sessão, versão, contadores, `HELLO`, descritor de firmware, `gen_seq`, confirmação, parâmetros alterados) e OTA pelo Joker Link |
| ref/10 | descritor e hash da geração (ref/10 §2.6.2), o trinco de transição de gerações partilhado com o agente, `/v1/agent/*` (§5.21), sessões remotas por túnel (`agent/1.3`) |

---

## 2. Transporte, identidade e papéis

### 2.1 Pontos de escuta

| Ponto | Omissão | Regras |
|---|---|---|
| `unix:/run/joker/api.sock` | sempre ligado | dono `joker:joker`, modo `0660`; o grupo do socket vem da configuração do nó (ref/02). Quem consegue abrir o ficheiro fala com a API |
| `tcp:<host>:<porta>` | desligado; porta de omissão 7443 | **TLS 1.3 com mTLS obrigatório**, sempre: não há chave para o desligar (a antiga `mtls` do manifesto é `J0140`, ref/01). Sem 0-RTT. Sem TLS sem certificado de cliente |

Os pontos vêm do `node.toml` (`[api] listen`, o mínimo que vale sempre, ref/02) e de `[security.api] listen` da geração, que **acrescenta** pontos (ref/01). Mudar os da geração é mudar de geração, e o plano marca a mudança como `api`, o que exige janela de confirmação em `prod` (ADR 0015; §5.13).

**Canal de arranque do enrolamento.** Num nó ainda não enrolado não há CA de clientes, e por isso não há mTLS. **Nunca há uma porta de enrolamento aberta à rede.** Os endpoints `/v1/node/enrollment/*` só respondem no **socket Unix**: com acesso físico ao nó, ou por SSH até ele (`--node ssh://…`, ref/02 §3.2.3). Sem código, o principal do socket tem de ser `root` ou do grupo `joker-admin` (por SSH, o utilizador SSH; ref/02 §3.2.3), senão `J-E1002`. Com o código de enrolamento de uso único que o nó mostra (`jokerd enroll-code`, só para `root` e `joker-admin`, ou a consola de uma imagem de fábrica; `jk node enroll --code`), basta um principal que chegue ao socket: a `jk` envia o código no cabeçalho `Joker-Enroll-Code` em cada pedido de enrolamento, e um código errado, expirado ou já usado é `J-E1913`. O código prova que quem enrola teve acesso ao nó; vale 15 min, uso único, 5 tentativas (ref/02 §3.2.3). O enrolamento num Deck usa o token de enrolamento, e é o nó que liga para fora (`POST /v1/node/enrollment/deck`, ref/10 §3.1.2).

**Do lado do cliente**, o `--node` da `jk` e das bibliotecas (ref/02 §3.3.1, ref/05 §22.1, documento 07 §1) aceita só estas formas: `local`, um nome de contexto do ficheiro do cliente (`[nodes]`), `unix:<caminho>`, `<host>:<porta>` (TCP com mTLS; porta 7443 por omissão) e `ssh://[<utilizador>@]<host>[:<porta>]` (túnel SSH até ao socket Unix). O prefixo `tcp:` só existe nos pontos de escuta do nó (esta tabela), e não no `--node`.

**HTTP:** HTTP/1.1 e HTTP/2. No socket Unix, HTTP/2 por *prior knowledge* (h2c); em TCP, ALPN `h2` e `http/1.1`. Sem HTTP/3. O cabeçalho `Host` é ignorado no socket Unix.

### 2.2 Identidade (principal)

Cada pedido tem um **principal**, que é também o escritor `user:<id>` das escritas manuais (ref/00, §5; documento 16, A1).

| Transporte | Como se autentica | `id` | `subject` guardado |
|---|---|---|---|
| `unix` | permissões do ficheiro + `SO_PEERCRED` (uid, gid, grupos) | nome do utilizador Unix | `uid`, nome |
| `tcp` | certificado de cliente que encadeia na CA de clientes do nó (ref/02) | `CN` do certificado | DN completo e SHA-256 do certificado |
| `tunnel` (1.3) | sessão remota aberta pelo Deck (`session-open-args`, ref/10 §2.12): o principal chega **pré-autenticado** | `deck:<deck_id>/user:<id>` (ref/00 §5) | `user-claim` do Deck (id, nome, grupos, MFA) e o `id` da sessão |

- O `id` tem a gramática `[A-Za-z0-9._@-]{1,64}` (no túnel, o `id` do `user-claim`). Um `CN` fora dela é recusado no primeiro pedido (`J-E1003`, 403).
- **O `id` não basta para comparar principais.** A ocupação de um nível manual, a libertação e a auditoria usam o principal completo (transporte + `subject` + impressão digital). Um certificado `CN=diogo` e o utilizador Unix `diogo` são principais diferentes.

### 2.3 Papéis

Três papéis, ordenados: `viewer` < `operator` < `admin` (ref/00, §6). Cada operação declara o papel mínimo (`x-joker-role` no OpenAPI e as tabelas do §5).

**Atribuição.** O papel é o **mais alto** de todas as regras de `[security.roles]` que casam com o principal (documento 03, §13; gramática na ref/01, §5.20):

```toml
[security.roles]
operator = ["cert:CN=ops-*", "local:group=joker-ops"]
admin    = ["cert:CN=admin-*", "local:user=deploy"]
```

| Regra | Casa com |
|---|---|
| `local:user=<nome>`, `local:group=<nome>` | utilizador ou grupo Unix do par (socket) |
| `cert:<ATTR>=<padrão>` | atributo do DN (`CN`, `OU`, `O`) ou `SAN` (URI, DNS, email), com `*` como glob |
| `deck:<deck_id>/user:<id>`, `deck:<deck_id>/group:<g>` | um utilizador ou um grupo do `user-claim` de uma sessão remota (ref/10 §3.10) |

- **Sem regra que case, o papel é `viewer`.** Chegar ao socket (permissões do ficheiro) ou apresentar um certificado válido da CA de clientes já é uma decisão de confiança; o papel só a sobe.
- **Implícitos no socket:** uid 0 e o utilizador `joker` são `admin`. Quem é root já tem o nó.
- **O papel no certificado** faz-se com uma regra `cert:SAN=urn:joker:role:operator` emitida pela CA da organização: o nó não confia num campo do certificado sem uma regra que o nomeie.
- **Túnel (1.3):** o papel efetivo é o **menor** de três: o pedido em `session-open-args`, `[agent.remote] max_role` do nó, e o que `[security.roles]` dá aos principais `deck:` (ref/10 §3.10; sem regra, `default_role`, e vazio recusa). **Proibido numa sessão, com qualquer papel** (`J0986`, 403): `/v1/node/*` (identidade, enrolamento e confiança), `/v1/agent/*` exceto `GET /v1/agent` e `GET /v1/agent/desired`, e `/v1/secrets`. Na 1.0 o transporte `tunnel` não existe.
- O papel é calculado em cada pedido HTTP e, numa ligação WebSocket ou SSE, na abertura e a cada troca de geração (§6.9).

### 2.4 Tokens de sessão, navegadores e CORS

- **A 1.0 não tem tokens de portador** (`Authorization: Bearer`). A identidade vem sempre do transporte. O Studio local (`jk ui`, 1.3) é um processo da `jk` que serve a interface em `localhost` e fala com o nó com as credenciais da própria `jk`; no Deck, fala pelo túnel.
- **CORS desligado.** Não há `Access-Control-Allow-*` em nenhuma resposta. Um pedido com cabeçalho `Origin` é recusado (`J-E1004`, 403): um navegador com o certificado de cliente instalado não pode ser usado por uma página qualquer contra o nó (CSRF, *DNS rebinding*). A lista de origens permitidas fica reservada para a 1.3.
- **Só `application/json`** nos corpos JSON. `application/x-www-form-urlencoded`, `multipart/form-data` e `text/plain` são `J-E1008` (415), o que fecha os pedidos "simples" de navegador.

### 2.5 Anti-replay (ref/00, §9)

As operações marcadas `x-joker-mutating` (todas as que mudam estado; §5) exigem, **em TCP e em túnel**:

| Cabeçalho | Formato | Regra |
|---|---|---|
| `Joker-Nonce` | ULID | não repetido nos últimos 60 s (duas vezes a janela) |
| `Joker-Timestamp` | RFC 3339 UTC | a ±30 s do relógio UTC do nó, com o relógio `synced` |

- Falta de um deles: `J-E1020` (400). Fora da janela: `J-E1021` (400). Nonce repetido: `J-E1022` (409).
- **No socket Unix são opcionais**, e verificados se vierem. O par é autenticado pelo kernel e não há rede onde capturar e repetir um pedido.
- **Relógio do nó `holdover` ou `unsynced`** (D16, ref/00 §9): o `Joker-Timestamp` não se pode verificar com confiança. O nó aceita o pedido só com a verificação do nonce e da ligação ao alvo, alarga a memória de nonces para 10 min, responde com `Joker-Clock: holdover` ou `unsynced` e a auditoria marca `replay_check = clock_holdover` ou `clock_unsynced`. Recusar tudo deixaria um operador sem mãos exatamente quando o NTP está em baixo.
- **A memória de nonces é limitada** (100 000 entradas). Cheia, os pedidos novos são recusados (`J-E1029`, 503) em vez de se esquecerem nonces: o anti-replay nunca falha aberto.
- **Nonce e idempotência são coisas diferentes.** Uma repetição legítima (a rede caiu antes da resposta) leva um **nonce novo** e a **mesma** `Idempotency-Key` (§3.8).

### 2.6 O que nunca passa pela API

| Coisa | Porquê | O que existe em vez disso |
|---|---|---|
| valor de um segredo, em qualquer sentido | ref/00, §9 | `GET /v1/secrets` dá nomes e revisões; a entrada aceita só entradas cifradas por HPKE para a chave KEM do nó (§5.3, D6) |
| a chave privada do nó | fica no TPM ou no ficheiro protegido | CSR e certificado (§5.3) |
| escrita que contorne o envelope | invariante 3 do `AGENTS.md` | escritas manuais, métodos e intenções entram no árbitro como qualquer escritor |
| o bit `force_safe` | só o supervisor o ativa (documento 01, §3.4) | leitura em `OutputState.force_safe` |
| escrita num nível fora de `manual` | as faixas são do manifesto (ref/01, §3.11) | `supervisor` chega pelo Deck (ref/10) ou por pontes |
| escrita no `kv` de uma app | o estado é da app | leitura, só `admin` |
| edição de ficheiros em `/var/lib/joker` | documento 15, §2.5 | gerações |
| mudança da política de confiança, dos papéis ou dos pontos de escuta fora de uma geração | seriam uma segunda verdade | gerações com confirmação |
| `SignalId` sem o hash do registo | ref/00, §5 | o `SignalId` só aparece em `SignalMeta` ao lado de `registry` |
| credenciais em URLs de uplinks, pontes e dispositivos | — | os endpoints aparecem sem credenciais |

---

## 3. Convenções da API

### 3.1 Versões

- Todos os caminhos começam por **`/v1`**. `info.version` do OpenAPI é a versão da API (`1.0.0`), e `GET /v1/version` diz que versões de todas as superfícies o nó suporta.
- **Numa minor só se acrescenta** (ADR 0022; ref/00, §1): endpoints, campos opcionais, valores novos no fim de um enum, tópicos novos. Nada se remove em 1.x. Um endpoint obsoleto leva `Deprecation` (RFC 9745) e continua a funcionar.
- **Respostas: o cliente ignora o desconhecido** (campos, valores de enum). Um valor de enum desconhecido conta como `unknown`, ou pela sua categoria (uma razão de qualidade desconhecida conta pela `quality`).
- **Pedidos: o servidor é estrito.** Um campo desconhecido no corpo, ou um parâmetro de query desconhecido, é `J-E1011` (400), exceto campos começados por `x_`. Um cliente 1.3 que use uma opção nova contra um nó 1.0 recebe um erro claro, em vez de ver a opção ignorada em silêncio (§11, caso S9). Os clientes consultam `GET /v1/info` (`features`) e `GET /v1/version` antes de usar o que é novo.

### 3.2 Cabeçalhos

| Cabeçalho | Sentido | Onde | Regra |
|---|---|---|---|
| `Joker-Request-Id` | resposta | todas | ULID; repetido no `instance` dos erros e na auditoria |
| `Joker-Generation` | resposta | todas | `"<gen> sha256:<hex>"` da geração em execução quando o pedido foi servido |
| `Joker-Clock` | resposta | todas | `synced`, `holdover`, `unsynced` |
| `Joker-Api-Version` | resposta | todas | `1.0.0` |
| `Joker-Nonce`, `Joker-Timestamp` | pedido | mutantes | §2.5 |
| `Idempotency-Key` | pedido | mutantes (`POST`) | §3.8; obrigatório em `POST /v1/actions/…` |
| `Joker-Idempotent-Replay` | resposta | idem | `true` quando a resposta é a guardada |
| `If-Match` | pedido | gerações, escritas | §3.9 |
| `ETag` | resposta | saídas, gerações | §3.9 |
| `traceparent` | pedido | todas | W3C Trace Context; propagado para os logs e o OTLP |
| `Retry-After` | resposta | 429, 503, 409 de trabalho em curso | segundos |
| `Location` | resposta | 201, 202 | recurso criado ou a acompanhar |

### 3.3 Representação JSON

Aplica-se a ref/00, §2–§6. O que esta API acrescenta:

| Coisa | Forma | Exemplo |
|---|---|---|
| instante | RFC 3339, `Z`, até 9 casas | `"2026-09-24T10:00:00.123456789Z"` |
| instante em parâmetros (`from`, `to`) | RFC 3339 (com `Z` ou deslocamento), `now`, ou relativo `-<duração>` | `from=-1h`, `to=now` |
| duração | literal (ref/00, §3.1). **Na saída**, inteiro na maior unidade exata de `d`, `h`, `m`, `s`, `ms`, `us`, `ns` | `"90m"`, `"1500ms"`, `"38us"` |
| monotónico | só com `mono=true`: `ts_acq_mono_ns`, `ts_hw_mono_ns` (strings decimais), mais `boot_id` na resposta | `"81234567890"` |
| valor | ref/00, §4.1, na unidade declarada do sinal | `412.5`, `"run"`, `"18446744073709551615"` |
| valor de entrada | número na unidade do sinal, literal de quantidade, booleano, variante, string decimal, base64 | `"50 kW"`, `50`, `true` |
| nível | `"<faixa>:<nível>"`; `"none"` sem nível ativo | `"manual:4"` |
| resultado de escrita | ref/00, §4.4 | `{"outcome": "clamped_rate", "applied": 1.0}` |
| qualidade | `quality` + `reason`, nomes da ref/00, §4.3 | `"uncertain"`, `"stale"` |
| geração | `{ "gen": 17, "hash": "sha256:…", "state": "confirmed" }` | |
| ids | ULID para pedidos, sessões, escritas, invocações, trabalhos, traços, intenções, uploads | |
| quantidades em metadados | literais tal como declarados, e o valor avaliado ao lado quando é uma expressão | `"range": ["-bms.p_max_discharge", "bms.p_max_charge"], "range_now": [-100, 100]` |
| tamanhos | prefixos IEC | `"16 MiB"` |

**A amostra** é a da ref/00, §4.2, com estes campos opcionais: `bits` (tipo `bits`), `age` (duração, calculada no momento da leitura), `acq_id` (só em leituras coerentes), `ts_acq_mono_ns`, `ts_hw_mono_ns`. A qualidade devolvida já inclui o envelhecimento (`Uncertain(Stale)` e `Bad(Stale)`), calculado no instante da leitura como no latch (documento 01, §3.3). `ts_hw` é traduzido para UTC como o `ts_acq`.

**Entrada de valores.** Um número é entendido na unidade do sinal. Um literal de quantidade é convertido; dimensão errada é `J0107`, grandeza errada é `J0108` (ref/00, §4.1). Um valor que não serve o tipo (texto num `bool`, variante inexistente, inteiro fora da gama do tipo) é `J-E1036`. `bool` só aceita `true` e `false`.

### 3.4 Erros

`application/problem+json` (RFC 9457), com os membros da ref/00, §8, e mais alguns:

| Membro | Tipo | Obrigatório | Conteúdo |
|---|---|---|---|
| `type` | URI | sim | `urn:joker:code:<código>` |
| `title` | string | sim | mensagem curta do registo `codes.toml` |
| `status` | inteiro | sim | o estado HTTP |
| `code` | string | sim | `J-E1102`, `J0107`, … |
| `detail` | string | não | o caso concreto |
| `instance` | string | não | `urn:joker:request:<ULID>` |
| `severity` | `error` \| `warning` | não | omissão `error` |
| `help` | string | não | como resolver, ao estilo do rustc |
| `pointer` | string | não | JSON Pointer (RFC 6901) para o campo do corpo |
| `param` | string | não | parâmetro de query ou cabeçalho |
| `spans` | lista | não | `{file, line, column, end_line?, end_column?, label?}` no manifesto efetivo |
| `errors` | lista | não | diagnósticos (`code`, `severity`, `message`, `help`, `key`, `spans`), por exemplo todos os J-codes de uma validação de geração |
| `suggestions` | lista | não | caminhos próximos por distância de edição (como o `J0101`) |
| `retry_after` | duração | não | igual ao `Retry-After` |
| `request_id` | ULID | não | igual ao `Joker-Request-Id` |

**Um resultado do envelope não é um erro HTTP.** Uma escrita manual rejeitada pelo permissivo responde **200** com `outcome: rejected, reason: interlock`: o pedido foi bem feito e a decisão foi tomada. O mesmo para uma ação que falhou no equipamento (`state: failed`). A `jk` traduz estes resultados nos seus códigos de saída (§9.2).

### 3.5 Paginação e ordem

- Listas paginadas: `?limit=` (1–1 000, omissão 100) e `?cursor=` (opaco). Resposta `{ "items": […], "next_cursor": "…" | null }`.
- O cursor é estável face a inserções (é uma posição, não um deslocamento) e vale 10 min.
- Ordem: sinais, saídas, alarmes, instâncias, dispositivos por **caminho ou nome** (ordem de bytes); decisões, eventos, logs, auditoria por **tempo** (`order=desc` por omissão; `asc` disponível).
- Um cursor inválido ou expirado é `J-E1032` (400).
- As respostas volumosas (histórico, decisões, logs, auditoria, eventos, transições de alarmes) aceitam `Accept: application/x-ndjson`: um objeto por linha, sem envelope, com os mesmos filtros; a paginação continua a valer pelo cabeçalho `Link: <…>; rel="next"`.

### 3.6 Seletores

A gramática do documento 02, §9, com esta precisão:

```
seletor  := parte ("." parte)*               (máx. 8 partes, 288 caracteres)
parte    := "**" | glob
glob     := [a-z*][a-z0-9_*]*                (máx. 32 caracteres)
```

- `*` casa com zero ou mais caracteres `[a-z0-9_]` **dentro de um segmento** (`inv*.ac.p`).
- `**` é um segmento inteiro e casa com **um ou mais** segmentos (`meter.**` casa `meter.p_total` e `meter.ac.p`, não `meter`).
- Um seletor casa com caminhos canónicos **e** aliases. Os resultados são deduplicados por `SignalId` e apresentados pelo caminho que casou primeiro, por ordem de bytes; o canónico vem em `SignalMeta.canonical`.
- Parâmetro `sel`, repetível, até 64 por pedido. Um seletor mal formado é `J-E1030`. Mais de 10 000 sinais resolvidos é `J-E1031` (422). Um seletor sem correspondências dá uma lista vazia (200), exceto nos endpoints de escrita, onde um caminho inexistente é `J-E1033` (404, com `suggestions`).
- Nos alarmes, os seletores aplicam-se ao id do alarme.

### 3.7 Intervalos de tempo

- `from`, `to` (formas do §3.3) e `since` (duração; equivale a `from=-<since>`). `since` com `from` é `J-E1034`. `from > to` é `J-E1034`.
- Omissões: `to = now`; `from` depende do endpoint (histórico: `to − 1h`; decisões, logs, auditoria, eventos: sem limite, paginado).
- As horas locais da `jk` (`--since "2026-09-24 03:00"`) são convertidas pela `jk`, com o fuso do cliente, antes de chegarem aqui. A API só recebe UTC ou deslocamentos explícitos.

### 3.8 Idempotência

| Onde | Chave | Janela | Âmbito | O que se guarda |
|---|---|---|---|---|
| `POST /v1/actions/{target}/{action}` | `Idempotency-Key` **obrigatório** (`J-E1023` sem ele) | `dedup_window` da ação (documento 16, E9; omissão 60 s) | (alvo, ação, chave), como no núcleo (documento 02, §8) | a invocação e o seu resultado |
| os outros `POST` mutantes | `Idempotency-Key` opcional | 10 min | (principal, operação, chave) | o estado HTTP e o corpo da resposta |

- Chave: ASCII imprimível, 1–64 B (ref/00, §5; `J-E1025` fora disso).
- A mesma chave com um corpo diferente (hash do corpo canónico) é `J-E1024` (422).
- A mesma chave com o pedido original ainda a correr é `J-E1028` (409, com `Retry-After`).
- A memória de respostas é limitada a 10 000 entradas por nó; ao encher, sai a mais antiga (a janela de 10 min é um máximo, não um mínimo, e isso está dito aqui para que ninguém dependa dela para mais do que repetições imediatas).
- As escritas manuais já são quase idempotentes (repetir renova o lease com o mesmo valor); a chave serve para não duplicar a entrada no registo de decisões e para devolver o mesmo bilhete.

### 3.9 Concorrência otimista (ETags)

- `ETag` de `GET /v1/generations/current` e de `GET /v1/outputs/{path}` é o **hash da geração** em execução, entre aspas (`"sha256:…"`). O envelope e o registo só mudam com a geração.
- `If-Match` com esse valor:
  - **obrigatório** em `activate`, `rollback`, `shadows` (início, promoção, aborto): `J-E1027` (428) sem ele, `J-E1026` (412) se a geração mudou;
  - **opcional** em `manual` e `methods`: a `jk` envia o que viu na pré-visualização, e uma geração nova entre a pré-visualização e a confirmação dá 412 em vez de aplicar um valor contra um envelope que a pessoa não viu.
- `GenerationProposal.base` é o mesmo mecanismo no corpo: o plano só é válido contra a geração `base` (`J-E1609`).

### 3.10 Limites de taxa

Balde de fichas por principal. Ao exceder: `J-E1014` (429) com `Retry-After`. Os valores são omissões; a configuração do nó (ref/02) pode baixá-los, nunca subi-los acima do dobro.

| Classe | Taxa | Rajada |
|---|---|---|
| leituras (`GET`, `POST …/query`, pré-visualizações) | 100/s | 200 |
| mutantes | 10/s | 20 |
| escritas manuais e métodos, por saída | 2/s | 4 |
| `POST /v1/values/refresh`, por sinal | 1/s | 1 |
| trabalhos (`doctor`, `discovery`, `profile-tests`, `bench`, `lock/probe`, `verify`) | 1 em curso por tipo e por nó | — |
| ligações WebSocket/SSE | 16 por principal, 1 000 por nó (documento 09, ataque 11) | — |

### 3.11 Limites

| Coisa | Limite | Código |
|---|---|---|
| corpo de um pedido | 16 MiB (ref/00, §10) | `J-E1009` (413) |
| bloco de upload | 8 MiB | `J-E1606` (413) |
| URL | 8 KiB; cabeçalhos 16 KiB | `J-E1009` |
| conteúdo em linha (`ContentRef.content`) | 1 MiB | `J-E1009` |
| seletores por pedido | 64 | `J-E1030` |
| sinais resolvidos por pedido | 10 000 | `J-E1031` |
| leitura coerente | 256 sinais | `J-E1037` |
| pontos de histórico por resposta | 100 000 | `J-E1038` (paginar ou agregar) |
| página | 1 000 itens | `J-E1012` |
| `reason` | 1 KiB (ref/00, §10) | `J-E1012` |
| mensagem WebSocket do cliente | 64 KiB | fecho 1009 |
| mensagem WebSocket do servidor | 1 MiB (os lotes partem-se) | — |
| subscrições por ligação | 64 | `J-E1903` |
| orçamento de amostragem do nó (todas as subscrições) | 1 000 000 amostras/s | `J-E1903` |
| bilhetes de escrita guardados | 10 min | `J-E1110` (404) |
| gerações guardadas para rollback | as 10 últimas | `J-E1613` |
| geração pendente não ativada | 1 h | `J-E1614` (410) |

### 3.12 Tipos de conteúdo

| Tipo | Onde |
|---|---|
| `application/json` | omissão |
| `application/problem+json` | erros |
| `application/x-ndjson` | listas volumosas (§3.5) |
| `text/csv` | `GET /v1/history`, `GET /v1/decisions` (colunas do §5.7 e §5.11) |
| `text/event-stream` | `GET /v1/stream` |
| `application/toml` | manifesto e lock de uma geração |
| `application/octet-stream` | blocos de upload, blobs |
| `application/vnd.joker.trace.v1` | `.jktrace` (documento 16, §7) |
| `application/yaml` | `GET /v1/openapi.yaml` |

As respostas aceitam `Accept-Encoding: gzip, zstd`. Os pedidos não são comprimidos.

### 3.13 Operações longas

Há três formas, e cada coisa usa a sua:

| Forma | Para quê | Recurso | Acompanhar |
|---|---|---|---|
| **bilhete de escrita** | escritas manuais, `release`, métodos, intenções aprovadas: esperam um tick do grupo dono | `/v1/writes/{id}` | `wait=true` (omissão, até `wait_timeout`, 5 s, máx. 30 s); senão 202 + `Location` |
| **invocação** | ações (M19), curtas ou longas | `/v1/invocations/{id}` | ações curtas: `wait=true` até ao `timeout` da ação; longas: 202, tópico `invocations` |
| **trabalho** | tudo o que demora e não é do núcleo: `doctor`, descoberta, testes de perfil, `bench`, `lock/probe`, `verify`, OTA da runtime e de MCUs | `/v1/jobs/{id}` | 202 + `Location`; tópico `jobs`; `POST /v1/jobs/{id}/cancel` |

Os trabalhos sobrevivem a um reinício do `jokerd` (o OTA da runtime reinicia-o por definição): o estado é persistido e, depois do arranque, o trabalho aparece `succeeded` ou `failed`.

---

## 4. Auditoria

Cada pedido mutante, cada recusa por papel, origem ou anti-replay, e cada abertura e fecho de sessão geram uma entrada de **auditoria** (`GET /v1/audit`, `admin`): principal completo, papel, sessão (a ligação HTTP e, no túnel, o `id` da sessão remota do Deck), `request_id`, operação, estado, código, resumo do pedido **sem segredos**, `nonce`, `Idempotency-Key` e resultado da verificação anti-replay. `session=<id>` filtra a auditoria de uma sessão remota (ref/10 §3.10). É guardada com `fsync = "always"`, como os trips e as entradas críticas do registo de decisões (documento 01, §12 e §19).

A auditoria diz **quem pediu o quê à API**. O **registo de decisões** (M3, §5.7) diz **o que o árbitro e o envelope fizeram** a cada escrita, venha de onde vier. Uma escrita manual aparece nos dois, ligada pelo `request_id`.

---

## 5. Referência dos endpoints

Legenda: **Papel** mínimo; **M** = mutante (anti-replay em TCP, §2.5). Os corpos e as respostas completos estão no OpenAPI, com os mesmos nomes de esquema entre parênteses.

### 5.1 Meta

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/info` | viewer | | nó, site, runtime (versão, `boot_id`, features do build), API, modo, placa, kernel, geração, relógio, limites, `features` da API, e quem chama (`Info`) |
| GET | `/v1/version` | viewer | | versões suportadas: API, mundo WIT atual e anterior, `joker:driver`, JDP, esquema do manifesto, lock, `agent/1`, Joker Link, ABI C, traço (`Version`) |
| GET | `/v1/health` | viewer | | `ok` \| `degraded` \| `failed` com verificações; `probe=live` (processo responde), `ready` (primeiro tick feito), `full`; 503 em `failed` (`Health`) |
| GET | `/v1/whoami` | viewer | | principal, papel, sessão, regras que casaram (`WhoAmI`) |
| GET | `/v1/status` | viewer | | resumo do `jk status`: geração (e prazo de confirmação), relógio, grupos, dispositivos, apps, alarmes (contagens e os 20 primeiros), trips, níveis manuais, uplinks, MCUs; `instances=true` acrescenta as instâncias (`Status`) |
| GET | `/v1/openapi.json`, `/v1/openapi.yaml` | viewer | | este contrato |

`features` na 1.0: `sse`, `intents` (a API; vazia até à 1.1), `shadow.apps`, `micro` (se houver dispositivos `joker-link`), `sim` (em `mode = "dev"` com dispositivos `sim`), `agent` (há agente configurado), `enroll.code` (há um código de enrolamento ativo, à espera de uso). Na 1.1 acrescentam-se `models`, `blocks`, `streams`, `schedule`; na 1.3, `tunnel`. É a lista que as bibliotecas cliente leem no `connect` (ref/05 §21).

### 5.2 Nó, grupos, métricas, logs

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/node` | viewer | | identidade (sujeito, emissor, impressão digital, validade, armazenamento da chave: `tpm` \| `secure_element` \| `file`), a cadeia do certificado em PEM (`chain_pem`, com a chave KEM no SAN URI) e a chave KEM (`kem_key`), enrolamento (`Node`). A cadeia serve para a `jk` atestar a chave KEM contra a CA da organização quando fala pelo socket, sem ligação mTLS (ref/02 §3.4.2) |
| POST | `/v1/node/enrollment/csr` | admin | M | gera (ou, com `rotate`, roda) a chave de identidade P-256 e, no primeiro enrolamento, a chave KEM X25519, e devolve um CSR PKCS#10 em PEM, com o nome do nó no CN e no SAN DNS e a chave KEM no SAN URI `urn:joker:kem:x25519:<base64url>` (ref/02 §3.2.3). **Só pelo canal de arranque** (§2.1: o socket Unix, local ou por SSH; senão `J-E1912`); `root` ou `joker-admin`, ou qualquer principal do socket com `Joker-Enroll-Code` |
| PUT | `/v1/node/enrollment/certificate` | admin | M | instala a cadeia emitida (folha primeiro), as âncoras de confiança (`anchors`, chaves `.pub` ou `trust-key`), o primeiro bundle (`trust`, COSE) e, opcionalmente, a CA de clientes da API e os parâmetros de `[policy]`; o `jokerd` escreve o `node.toml` (ref/02 §3.2.3, passo 4). A folha tem de corresponder à chave (`J-E1911`) e ter a chave KEM no SAN. Só pelo canal de arranque |
| POST | `/v1/node/enrollment/deck` | admin | M | enrola num Deck (ref/10 §3.1.2): `deck_url`, `token`, `force`; o nó faz o `POST /enroll` e instala a resposta → trabalho `enroll`. Só pelo canal de arranque |
| POST | `/v1/node/renew` | admin | M | renova já o certificado (pelo Deck, ou devolve um CSR com a **mesma** chave KEM, sem Deck) → trabalho `renew` |
| GET | `/v1/node/hardware` | viewer | | inventário do hardware do nó, **só com leituras** do sistema de ficheiros (`/proc`, `/sys`, `/dev`): arquitetura, modelo, CPUs, memória, kernel, linha de arranque, chips GPIO, LEDs, zonas térmicas, watchdogs, portas série, I²C, SPI, rede, USB, TPM (`NodeHardware`). É o que o `jk board probe` e o `jk board check --node` usam (ref/03 §2.3.7) |
| GET | `/v1/groups`, `/v1/groups/{group}` | viewer | | período, CPUs, prioridade, política de overrun, trip, ordem, tarefas (WCET declarado, do lock, medido), jitter e duração (p50, p99, p99,9, máx.), cobertura do anel de traço (`Group`) |
| GET | `/v1/metrics` | viewer | | instantâneo das métricas do documento 01, §19, com os histogramas que não cabem em sinais escalares; `ml` é `null` na 1.0 (`Metrics`) |
| GET | `/v1/logs` | viewer | | logs estruturados do anel da API (16 MiB por omissão), filtros `source`, `app`, `level` (mínimo), intervalo; para seguir, tópico `logs` (§6) |

**`NodeHardware`** (`GET /v1/node/hardware`). Cada campo diz de onde vem; um campo que o nó não consegue ler (permissões, sistema sem `device-tree`) vem `null`, e a lista `unreadable` diz qual e porquê. Nada aqui abre um dispositivo para escrita, pede um GPIO ou toca num barramento: o `jokerd` lê diretórios e atributos, e as linhas GPIO só pelo `ioctl` de informação do chardev (`GPIO_V2_GET_LINEINFO_IOCTL`), que não as reserva.

| Campo | Tipo | Fonte | Significado |
|---|---|---|---|
| `collected_at` | `Timestamp` | — | quando o inventário foi lido (é lido a cada pedido; não há cache) |
| `arch` | string | `uname -m`, normalizado | `aarch64`, `x86_64`, `armv7`, `armv6`, `riscv64` (os valores de `[board] arch`, ref/03 §2.3.1); `machine` traz o valor cru (`armv6l`) |
| `model` | string \| `null` | `/proc/device-tree/model` (sem o `\0` final); em x86, `/sys/class/dmi/id/product_name` | `"Raspberry Pi Zero W Rev 1.1"` |
| `compatible` | lista de strings | `/proc/device-tree/compatible` | `["raspberrypi,model-zero-w", "brcm,bcm2835"]` |
| `cpus` | objeto | `/sys/devices/system/cpu` | `count` (online), `possible`, `model` (`/proc/cpuinfo`), `max_freq` (`Quantidade`, de `cpufreq/cpuinfo_max_freq`), `isolated` (lista, `/sys/devices/system/cpu/isolated`), `nohz_full` (lista) |
| `memory` | objeto | `/proc/meminfo` | `total` (`MemTotal`, `Tamanho` IEC), `available` |
| `kernel` | objeto | `uname -r`, `/sys/kernel/realtime`, `uname -v` | `release`, `version`, `preempt_rt` (bool: `/sys/kernel/realtime` = 1, ou `PREEMPT_RT` em `uname -v`), `preempt` (`none` \| `voluntary` \| `full` \| `rt`, de `/sys/kernel/debug/sched/preempt` se legível, senão de `uname -v`) |
| `cmdline` | lista de strings | `/proc/cmdline` | as linhas de arranque, separadas; `boot_params` destaca as que o `jk doctor --rt` conhece (`isolcpus`, `nohz_full`, `rcu_nocbs`, `irqaffinity`) |
| `gpio_chips` | lista | `/dev/gpiochip*` (ioctl de informação) | `path`, `name` (`gpiochip0`), `label` (`pinctrl-bcm2835`), `lines` (número de linhas), `used` (lista de `{ offset, name, consumer }` das linhas já pedidas pelo kernel ou por outro processo, p. ex. o LED `ACT` pedido pelo `leds-gpio`), `named` (quantas linhas têm nome) |
| `leds` | lista | `/sys/class/leds/*` | `name` (`ACT`, `PWR`), `max_brightness`, `trigger` (o ativo, entre `[ ]`: `mmc0`, `heartbeat`, `none`), `gpio` (`{ chip, offset }` quando o LED é de `leds-gpio`, pelo `device/of_node`) |
| `thermal_zones` | lista | `/sys/class/thermal/thermal_zone*` | `name` (`thermal_zone0`), `type` (`cpu-thermal`), `temp` (`Quantidade` em `°C`, lida agora) |
| `watchdogs` | lista | `/sys/class/watchdog/watchdog*` | `device` (`/dev/watchdog0`), `identity` (`bcm2835-wdt`), `timeout`, `min_timeout`, `max_timeout` (`Duração`; `null` se o driver não os expõe), `nowayout` (bool), `active` (bool: `state` = `active`, alguém já o armou) |
| `serial` | lista | `/sys/class/tty/*` com `device`, `/dev/serial/by-id`, `/dev/serial/by-path`, ligações `/dev/serial0`, `/dev/serial1` | `path` (`/dev/ttyAMA0`, `/dev/ttyUSB0`), `aliases` (as ligações estáveis que apontam para ela: `/dev/serial0`, `/dev/serial/by-id/usb-Silicon_Labs_CP2102_…-if00-port0`), `driver` (`uart-pl011`, `cp210x`), `usb` (`{ vendor_id, product_id, serial }` se for USB), `soc` (bool: é uma UART da placa, e não um adaptador) |
| `i2c` | lista | `/dev/i2c-*`, `/sys/class/i2c-adapter` | `path`, `name` (o do adaptador: `bcm2835 (i2c@7e804000)`). **Não** se faz varrimento de endereços |
| `spi` | lista | `/dev/spidev*` | `path`, `bus`, `cs` |
| `net` | lista | `/sys/class/net/*` | `name` (`eth0`, `wlan0`), `kind` (`ethernet` \| `wifi` \| `loopback` \| `virtual` \| `other`: `wireless/` ou `phy80211` dá `wifi`; sem `device` dá `virtual`), `mac`, `up` (bool), `usb` (bool: a interface está atrás de USB, como a Ethernet do Pi 3), `addresses` (IPv4 e IPv6 atribuídos) |
| `usb` | lista | `/sys/bus/usb/devices/*` com `idVendor` | `bus`, `port`, `vendor_id`, `product_id`, `manufacturer`, `product`, `class`; nunca o número de série de dispositivos que não sejam portas série |
| `tpm` | objeto | `/sys/class/tpm/tpm0` | `present` (bool), `version` (`"2.0"`), `device` (`/dev/tpmrm0`) |
| `unreadable` | lista de `{ field, path, reason }` | — | o que não se conseguiu ler, e porquê (`permission`, `absent`, `error`) |

- **Só leituras, e nada sensível.** O inventário é `viewer` porque não mexe em nada nem revela segredos; não inclui números de série de equipamentos além das portas série USB (é por eles que se escreve o caminho `by-id`), nem o conteúdo de ficheiros do nó.
- **Não é a placa.** O inventário diz o que existe **agora** neste nó; o perfil de placa diz o que o **modelo** garante. Um adaptador USB-série ligado aparece em `serial`, mas não pertence à placa: o `jk board probe` põe-no num comentário, e não em `[serial]` (ref/03 §2.3.7).
- Em `mode = "prod"` responde da mesma forma; o custo é o de ler algumas centenas de ficheiros em `/sys`, e corre em `bg`.

O `jk node enroll <alvo> --ca <…>` corre no nó ou chega ao socket por SSH (`ssh -L`), com `--code` quem não for `root` nem do grupo `joker-admin` no nó; gera o CSR, assina-o com a CA local da organização e instala o certificado e as âncoras. Com `--deck`, usa `POST /v1/node/enrollment/deck` e o protocolo do agente (ref/10 §3.1.2). A confiança depois do enrolamento está no §5.21. Os métricos também são sinais `joker.*` (documento 01, §19) e leem-se com `/v1/values?sel=joker.**`.

### 5.3 Segredos, sessões, auditoria

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/secrets` | admin | | nomes, `rev`, tipo, `signer`, se estão definidos, se são declarados, quem os usa, quando mudaram; **nunca o valor** (`SecretEntry`). É o que o `jk secret status` compara com o ficheiro (ref/02 §2.4.5) |
| POST | `/v1/secrets` | admin | M | recebe um `secret-bundle` (tipo `joker.secrets.v1`, `application/vnd.joker.secrets.v1+cbor`, `spec/cddl/secrets.cddl`) com as entradas cifradas para este nó (`jk secret push`, e o `jk apply` junto com a geração). Cada entrada é verificada sozinha (ref/02 §3.4.3: assinatura do papel `secrets`, `wrap` para este nó, decifração de teste, `rev`); a resposta diz, por nome, `accepted` ou o código (`J0922`–`J0926`) (`SecretPushResult`) |
| PUT | `/v1/secrets/{secret}` | admin | M | uma só entrada, em JSON (`SealedSecret`: `scheme` = a suite HPKE da ref/02 §2.4.4, `key_id` = o id da chave KEM do nó, `sealed` = o CBOR da `secret-entry` em base64); as mesmas verificações. Um segredo que nenhuma geração declara é `J-E1921`; uma entrada inválida é `J-E1920` com o código da ref/02 em `errors` |
| DELETE | `/v1/secrets/{secret}` | admin | M | apaga (só um segredo que nenhuma geração guardada refere) |
| GET | `/v1/sessions` | admin | | sessões ativas (HTTP persistente, WebSocket, SSE) |
| DELETE | `/v1/sessions/{session}` | admin | M | fecha uma sessão (WebSocket com 4009) |
| GET | `/v1/audit` | admin | | auditoria (§4); filtros `principal`, `operation`, `outcome`, intervalo |

### 5.4 Sinais e valores

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/signals` | viewer | | registo da geração: `sel`, `direction`, `origin`, `device`, `instance`, `shape`, `values`; paginado; traz `generation` e `registry` (`SignalMeta`) |
| GET | `/v1/signals/{path}` | viewer | | metadados e amostra de um sinal, por caminho canónico ou alias |
| GET | `/v1/signals/{path}/lineage` | viewer | | driver, dispositivo, barramento, perfil e registo, aquisição (escala, unidades, deadband), leitores (apps, `calc`, alarmes, permissivos, `requires_good`, gamas, consultas, pontes, uplinks), escritores, grupo dono, readback, grupo atómico (`Lineage`) |
| GET | `/v1/values` | viewer | | amostras atuais (`sel` obrigatório); `coherent=true` faz uma leitura coerente (documento 02, §6.4) e acrescenta `acq_id`; `mono=true` (`ValuesResponse`) |
| POST | `/v1/values/query` | viewer | | o mesmo com o corpo `{sel, coherent, mono}` |
| POST | `/v1/values/refresh` | operator | M | força a leitura dos sinais `param` com `poll = "on_demand"` que casam com `sel` (documento 05, §3) e devolve as amostras novas; os outros sinais do seletor são ignorados; timeout (omissão 5 s) é `J-E1016` |

`SignalMeta`: `path`, `canonical`, `aliases`, `signal_id` (válido só com `registry`), `type`, `enum` (variante → valor), `bits` (nome → posição), `unit`, `quantity`, `shape` (`scalar`, `event`; `block` e `stream` reservados para a 1.1), `direction` (`input`, `output`, `param`, `internal`), `origin` (`device`, `app`, `calc`, `alarm`, `bridge`, `pipeline`, `runtime`, `instance`, com o nome), `max_age`, `poll`, `limits`, `stuck_detect`, `record`, `writable`, `group`, `members`, `sample`.

**`refresh` pede `operator`** porque gera tráfego no barramento. `viewer` é um papel passivo: nada do que faz chega a um equipamento.

### 5.5 Historian e eventos

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/history` | viewer | | consulta crua ou agregada (`HistoryResponse`) |
| POST | `/v1/history/query` | viewer | | o mesmo com o corpo `HistoryQuery` |
| GET | `/v1/events` | viewer | | ocorrências gravadas de sinais com forma `event` (`EventRecord`: `path`, `ts`, `ts_src`, `seq`, `record`) |

Parâmetros do histórico:

| Parâmetro | Tipo | Omissão | Validação | Erro |
|---|---|---|---|---|
| `sel` | seletores | — (obrigatório) | §3.6 | `J-E1030`, `J-E1031` |
| `from`, `to`, `since` | §3.7 | `to − 1h` .. `now` | §3.7 | `J-E1034` |
| `step` | duração | sem `step`: pontos crus | ≥ 1 ms; com `tier` fixo, ≥ resolução do tier | `J-E1039` |
| `agg` | lista de `avg`, `min`, `max`, `first`, `last`, `count`, `quality` | com `step`: `avg,min,max,count,quality` | `avg`, `min`, `max` só em tipos numéricos | `J-E1039` |
| `tier` | `auto`, `raw`, `1s`, `1m`, `15m` | `auto` | — | — |
| `fill` | `none`, `previous`, `null` | `none` | — | — |
| `limit`, `cursor` | pontos por página | 10 000 | ≤ 100 000 | `J-E1038` |

- Com `tier=auto`, o nó escolhe o tier mais grosso com resolução ≤ `step` que cobre `from`. Se nenhum cobrir o intervalo todo, a resposta traz o que há, com `coverage` (tier, de, até) e um aviso.
- `quality` num agregado é a **qualidade dominante** do tier (documento 01, §12).
- Pontos crus: `ts` (o `ts_acq` em UTC), `ts_src`, `value`, `quality`, `reason`, `seq`.
- A tradução monotónico → UTC usa a tabela de correspondências do historian (documento 01, §18). **Uma correção do relógio pode mudar, retroativamente, os `ts` devolvidos** para amostras antigas; os valores e os `seq` não mudam.
- `text/csv`: colunas `path,ts,value,quality,reason,seq` (crus) ou `path,ts,<agregados>,quality` (agregados).
- **Parquet é produzido pela `jk`** (`jk export --parquet`), a partir de NDJSON. O nó não leva um escritor de Parquet (tamanho do binário, CPU em `bg`).

### 5.6 Saídas, árbitro e escritas manuais

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/outputs` | viewer | | saídas com valor efetivo, aplicado, nível ativo, último resultado, trip; filtros `sel`, `tripped`, `manual` (`OutputState`) |
| GET | `/v1/outputs/{path}` | viewer | | estado completo: envelope (com `range_now` e `actuations_used`), níveis ocupados, escritores declarados, `force_safe`, readback; `ETag` |
| POST | `/v1/outputs/{path}/preview` | viewer | | pré-visualização de uma escrita manual; **não escreve** (`WritePreview`) |
| POST | `/v1/outputs/{path}/manual` | operator | M | escrita manual (`ManualWriteRequest` → `WriteTicket`) |
| POST | `/v1/outputs/{path}/release` | operator | M | liberta um nível manual (`ReleaseRequest`) |
| GET | `/v1/manual` | viewer | | todos os níveis manuais ativos (`ManualOverride`) |
| GET | `/v1/writes/{write}` | viewer | | bilhete de uma escrita (10 min) |

**Corpo `ManualWriteRequest`** (também o da pré-visualização):

| Campo | Tipo | Obrig. | Omissão | Validação | Erro |
|---|---|---|---|---|---|
| `value` | valor de entrada | sim | — | §3.3; `null` não é aceite | `J-E1036`, `J0107`, `J0108` |
| `ttl` | duração | **sim** (A10) | — | 1 s ≤ `ttl` ≤ `max_manual` da saída (omissão 8 h) | `J-E1101`, `J-E1103`, `J-E1102` |
| `level` | `manual:3`–`manual:5` | não | `manual:4` | só a faixa `manual` (ref/01, §3.11) | `J-E1104` |
| `reason` | string, 1–1 024 B | sim | — | — | `J-E1106` |
| `takeover` | bool | não | `false` | — | `J-E1105` sem ele, se o nível for de outro principal |
| `wait` | bool | não | `true` | — | — |
| `wait_timeout` | duração | não | `5s` | ≤ `30s` | `J-E1012` |
| `at` | instante | não | — | reservado para a 1.1 (M6) | `J-E1017` |

**Semântica (documento 16, A1–A11):**

1. O escritor é `user:<id>` do principal, `kind = manual`, no nível pedido. O `lease_until` é `t_tick + ttl` (16 A2, com o tick em que a escrita entra); o TTL de uma escrita manual **é** o seu lease.
2. A proposta vai para a inbox do **grupo dono** da saída (A11) e é aplicada no tick seguinte desse grupo, como uma escrita async (documento 01, §5): passo 2 do tick, antes das tarefas de ciclo (A4).
3. **Renovar:** o mesmo principal no mesmo nível substitui valor e TTL (`renewed: true`).
4. **Ocupação:** outro principal no mesmo nível dá `J-E1105` (409) com quem o ocupa, até quando e porquê. Com `takeover: true`, a API envia para a mesma inbox, pela ordem, o *relinquish* do ocupante e a proposta nova; o registo de decisões guarda as duas (`kind = takeover`). Não há empates num nível (A5).
5. **Resposta:** com `wait`, 200 quando o tick do grupo dono fez o commit, com o `WriteOutcome`. Um `rejected`, `clamped_*`, `suppressed` ou `overridden` é uma resposta 200. Sem `wait`, ou se o tick não chegar em `wait_timeout` (grupo encravado), 202 com o bilhete em `queued`.
6. **`If-Match`** com o hash da geração vista na pré-visualização: 412 se a geração mudou entretanto.
7. **Aviso de expiração** (documento 01, §6, regra 5): 15 min antes do fim do TTL, o alarme `joker.output.<saída>.manual_expiring` (gravidade `medium`) e um evento no tópico `manual`. Com `ttl ≤ 15 min`, só o evento: um alarme imediato seria ruído. Quando o TTL acaba, o nível liberta-se sozinho e o registo de decisões guarda `kind = expiry`.
8. **Reinícios** (documento 16, A14): o nível manual persiste com o prazo em UTC e é reposto antes do primeiro tick. Se o relógio não estiver `synced` no arranque, repõe-se o TTL restante do último registo, contado em monotónico, e levanta-se o alarme `joker.output.<saída>.manual_restored` (gravidade `medium`) e um evento `restored` no tópico `manual`, para que um operador confirme (renovando) ou liberte. O `ManualOverride` mostra-o com `restored: true` até à renovação ou à libertação.

**Pré-visualização (`WritePreview`).** A `jk write` mostra-a antes de pedir confirmação (documento 07, §1, ponto 3, e §4):

| Campo | Conteúdo |
|---|---|
| `current` | o `OutputState` agora |
| `proposal` | nível, valor **convertido para a unidade da saída**, TTL, `lease_until`, `renews` |
| `conflict` | `null`, ou quem ocupa o nível pedido (escritor, principal, até, motivo) |
| `prediction.wins` | o nível pedido passa a ser o ativo |
| `prediction.overrides` | níveis ativos de menor prioridade que deixam de mandar |
| `prediction.outcome` | resultado previsto: `overridden(level)` se houver um nível mais alto ativo; senão, o envelope avaliado sobre os valores atuais (relógio, permissivo, `requires_good`, gama, taxa face ao último aplicado, deadband, `min_interval`, orçamento de atuações, grupo atómico) |
| `prediction.first_applied`, `time_to_target` | primeiro valor aplicado e estimativa com a taxa declarada |
| `prediction.inputs` | as amostras usadas nas expressões |
| `envelope` | o envelope declarado |

A previsão usa as mesmas funções do núcleo sobre os valores atuais, fora de um tick: é **indicativa**. O resultado que conta é o do tick (§8.2).

**Libertar (`ReleaseRequest`).** `level` (omissão: o nível que o principal ocupa nessa saída, se for um só; `J-E1012` se forem vários), `force` (liberta um nível de outro principal; fica na auditoria; `J-E1108` sem ele), `reason`, `wait`. Um nível que não está ocupado é `J-E1107` (404). Libertar é `relinquish()` (documento 01, §6, regra 3).

### 5.7 Trips e registo de decisões

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/trips` | viewer | | saídas em trip: desde quando, `scope` (`output` ou `group`), causa (`range`, `alarm`, `group_overrun`, `runtime`), `reset_when` e o seu valor atual (`Trip`) |
| POST | `/v1/outputs/{path}/reset` | operator | M | reset de um trip (`ResetRequest`: `reason` obrigatório, `wait`) |
| POST | `/v1/trips/reset` | operator | M | reset de várias saídas por seletor (`BulkResetRequest`), por exemplo depois do trip de um grupo |
| GET | `/v1/decisions` | viewer | | registo de decisões (M3): uma entrada por mudança de (valor aplicado, resultado, nível ativo), com `repeats` e `last_ts` (01 §19) |

**Reset (documento 16, E7).** O pedido é uma ação `manual` que vai para a inbox do grupo dono. A resposta vem depois do tick seguinte: `cleared`, ou `retripped` (a causa ainda está presente e a saída voltou ao trip nesse tick, com a causa). Uma saída que não está em trip é `J-E1120` (409). O trip é persistente (`state/trips`, documento 01, §7), e o reset também é gravado com `fsync`.

**Filtros do registo de decisões:** `output` (seletores), `cause` (só as decisões cujo conjunto causal contém esse sinal: `jk decisions --cause`), `writer`, `outcome`, `kind` (`proposal`, `expiry`, `release`, `takeover`, `safe`, `trip`, `reset`, `method`, `intent`), intervalo, ordem, paginação.

**`Decision`:** `id`, `ts`, `tick` (grupo, seq), `generation`, `output`, `kind`, `writer` (id, kind, nível, lease), `on_behalf_of`, `proposed`, `outcome`, `applied`, `active_level`, `inputs` (conjunto causal: caminho, seq, valor, qualidade), `app` (nome, versão, digest), `manual` (motivo, TTL, `request_id`, principal), `method`, `intent`, `model_outputs` (reservado para a 1.1, M22).

`text/csv` (`jk decisions --report csv`): `ts,output,kind,writer,level,proposed,outcome,reason,applied,active_level,repeats,last_ts,principal,request_id`. O relatório em PDF é gerado pela `jk` a partir do CSV; o nó não produz PDF.

### 5.8 Alarmes

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/alarms` | viewer | | alarmes; `sel` (sobre o id), `state` (`active`, `unacked`, `shelved`, `suppressed`, `all`), `severity` mínima (`Alarm`) |
| GET | `/v1/alarms/{alarm}` | viewer | | um alarme |
| POST | `/v1/alarms/{alarm}/ack` | operator | M | reconhece (`AckRequest`: `reason` opcional) |
| POST | `/v1/alarms/{alarm}/shelve` | operator | M | arquiva (`ShelveRequest`: `ttl` e `reason` obrigatórios) |
| POST | `/v1/alarms/{alarm}/unshelve` | operator | M | desarquiva |
| GET | `/v1/alarm-events` | viewer | | transições, reconhecimentos, arquivamentos, fins de arquivamento, supressões (`AlarmEvent`) |

- **Id do alarme:** o nome do manifesto (`grid_overfreq`), o da runtime (`joker.device.pcs.offline`, `joker.group.fast.overrun`, `joker.output.<saída>.manual_expiring`, `joker.output.<saída>.manual_restored`; a lista completa dos `joker.*` está na ref/01 e no documento 01, §19) ou o de uma app (`app.<app>.<nome>`, ref/04 §4.2; os sinais são `alarm.app.<app>.<nome>.state` e `.active`).
- **Ack** segue o documento 02, §11: `active_unacked` → `active_acked`; `rtn_unacked` → `normal`. Em `active_acked` é idempotente (200, sem mudança). Em `normal` e `unknown` é `J-E1204` (409): não há nada para reconhecer, e um `unknown` é um problema de qualidade das entradas, com o seu próprio alarme.
- **Shelve:** `ttl` obrigatório (documento 02, §11; `J-E1201`), no máximo `max_shelve` do alarme (omissão **24 h**, ref/01 §5.15; `J-E1202`). Arquivar não muda o estado ISA-18.2 do alarme: suprime a notificação (tópicos, uplinks com prioridade `alarms`) e marca `shelved`. No fim do TTL, `shelve_expired` e o alarme volta a notificar com o estado em que estiver. Desarquivar um alarme não arquivado é `J-E1203`.
- `Alarm`: `id`, `source` (`manifest`, `app`, `runtime`, `instance`), `severity`, `state`, `active`, `uncertain`, `suppressed`, `shelved` (até, por quem, motivo), `acked_by`, `acked_at`, `since`, `message` (já com os valores interpolados), `condition`, `signals` (`state`, `active`), `trips`, `max_shelve`.

### 5.9 Intenções (M18)

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/intents` | viewer | | por omissão só as `pending`; `state`, `output` (`Intent`) |
| GET | `/v1/intents/{intent}` | viewer | | uma intenção; `preview=true` acrescenta a pré-visualização da escrita que a aprovação faria agora |
| POST | `/v1/intents/{intent}/approve` | operator | M | aprova e executa (`ApproveIntentRequest`) |
| POST | `/v1/intents/{intent}/reject` | operator | M | rejeita (`reason` obrigatório) |

- Uma intenção nasce de `propose(saída, valor, justificação, ttl)` de um escritor `kind = "proposal"` (documento 01, §6.1; ref/04). Fica `pending` até `expires_at = created_at + ttl`; se ninguém decidir até lá, passa a `expired`. Uma intenção vale o tempo que o proponente disse que valia.
- **Aprovar** executa uma escrita manual com o escritor `intent:<id>` (ref/00, §5), `on_behalf_of` = quem aprovou, no nível `level` (omissão `manual:4`), com TTL = o da intenção ou menos (`ttl` pode encurtar; alongar é `J-E1302`), limitado a `max_manual`. As regras de ocupação e `takeover` do §5.6 aplicam-se. O resultado vem em `Intent.write`.
- Uma intenção que já não está `pending` é `J-E1301` (409).
- **Versão:** a API de intenções existe na 1.0. Os escritores só-proposta entram na 1.1 (documento 11), e até lá a lista está sempre vazia (§12).

### 5.10 Ações (M19)

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/actions` | viewer | | catálogo: alvo e tipo (`device`, `instance`), ação, argumentos (tipo, unidade, opcional), permissivo e o seu valor atual, papel, timeout, longa, `dedup_window` (`ActionDecl`) |
| POST | `/v1/actions/{target}/{action}` | operator (ou o papel da ação, se maior) | M | invoca; `Idempotency-Key` obrigatório (`InvokeRequest` → `Invocation`) |
| GET | `/v1/invocations`, `/v1/invocations/{invocation}` | viewer | | estado, progresso, resultado |
| POST | `/v1/invocations/{invocation}/cancel` | operator | M | cancela uma ação longa |

- Dispositivos e instâncias partilham o espaço de nomes do primeiro segmento (documento 16, P3), e por isso `target` é um só.
- **Corpo:** `args` (objeto por nome; quantidades aceitam literais), `timeout` (pode encurtar o configurado, não alongar), `reason`, `wait`.
- **Ordem (documento 17, §4.2):** papel → permissivo → deduplicação pela chave → fila serializada do dispositivo → passos do perfil ou `on-action` da implementação virtual → resultado.
  - papel abaixo do declarado em `[devices.*.actions.*]` ou `[instances.*.actions.*]`: `J-E1002` (403). **O chão é `operator`**: uma ação declarada com `role = "viewer"` exige `operator` na API (a ref/01 dá o aviso no `jk check`);
  - permissivo falso: `J-E1402` (409), com a expressão e os valores;
  - fila do dispositivo cheia: `J-E1403` (503);
  - argumentos inválidos: `J-E1401` (400).
- **Resultado:** uma ação curta responde 200 com `state` `succeeded` (e `result`) ou `failed` (e `error`: `kind` `denied` \| `timeout` \| `device` \| `invalid` \| `cancelled`, `code` `J-E1406`–`J-E1409` ou `J-E1415`, `message`). Uma ação longa responde 202 com o `ActionHandle` em `/v1/invocations/{id}`; o progresso chega no tópico `invocations`. Os códigos são o par dos que uma app recebe num `invoke` (tabela da ref/04 §7.1): o mesmo facto, visto por cada lado.
- **Deduplicação:** a mesma chave dentro de `dedup_window` devolve a invocação anterior (200, `replay: true`, `Joker-Idempotent-Replay: true`); com argumentos diferentes, `J-E1024`.
- Cancelar uma ação curta ou terminada é `J-E1405` (409).
- As ações de dispositivos `joker-link` viajam como `CALL`/`ACTION` (documento 19, §6; ref/07).

### 5.11 Instâncias e classes (M24)

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/instances` | viewer | | instâncias: classe, versão resolvida, hash do contrato, origem (`implicit`, `composed`, `virtual`) e fonte, capacidades, saúde, implementação (`Instance`) |
| GET | `/v1/instances/{instance}` | viewer | | membros (`in`, `out`, `param`, `event`, `action`, `method`, `query`, `member`) com caminho, mapeamento e amostra; a **árvore** dos membros de classe, com a instância ligada, a classe, a origem e a saúde de cada nó (`InstanceNode`); as composições de que a instância é membro (`member_of`); `explain=true` acrescenta a origem de cada mapeamento e a latência de cada camada virtual (`jk explain <instância>`) |
| POST | `/v1/instances/{instance}/methods/{method}/preview` | viewer | | expansão prevista do método e resultado previsto de cada `out` (`MethodPreview`) |
| POST | `/v1/instances/{instance}/methods/{method}` | operator | M | chama o método como escritor `manual` (`MethodCallRequest` → `MethodCallResult`) |
| POST | `/v1/instances/{instance}/verify` | admin | M | corre a conformidade da classe contra a instância real (trabalho `class_verify`): `allow_actuation` obrigatório numa instância não simulada (`J0640`), recusa com um nível `safety` ou `manual` ativo nas saídas (`J0641`), escreve em `manual:3` com TTL = duração da suite (ref/03 §2.6.3) |
| GET | `/v1/classes` | viewer | | classes da geração em execução, com versão e contrato |
| GET | `/v1/classes/{package}/{class}` | viewer | | o `contract.json` (documento 17, §6); `std/pump@1` → `/v1/classes/std/pump@1` |

- **Método** (documento 17, §4.1): `args` por nome, `ttl`, `level`, `reason`, `takeover`, `wait`, `wait_timeout`, com as mesmas regras do §5.6. O núcleo guarda um registo de método no nível do chamador e expande-o no passo 5a do tick do grupo dono. A resposta traz um bilhete por `out` expandido. Um método que escreve `out` de grupos diferentes produz uma escrita por grupo, e a resposta espera por todas. Método inexistente: `J-E1411`; argumentos inválidos: `J-E1412`.
- **Saúde** (documento 17, §4.5): `ok`, `degraded`, `failed`, com os membros responsáveis em `health_detail`.
- **Árvore** (documento 17, §8): um membro de classe aparece em `members` com `kind = "member"`, a `class`, os `bounds` numa coleção e `elements`, um `InstanceNode` por instância ligada, pela ordem da coleção. Cada nó tem o `path` aninhado (`esp_io.boot`, `station.pumps.p1`), a `instance` (nula num submembro de perfil sem nome), a saúde e os seus próprios `members`, recursivamente, até à profundidade 3. A saúde de topo já é a da árvore. Com `values=false`, os nós vêm sem amostras. Uma instância virtual com a implementação parada mostra `implementation.state = stopped` (a instância `sinal` da trilha, parte 8, se a `status_light_ctl` parar).
- **Verify:** `conformance` (blob ou conteúdo), `cases`, `level` (omissão `manual:3`: o harness escreve como um operador), `max_duration`, `allow_prod`, `reason`. Em `mode = "prod"` exige `allow_prod: true` (`J-E1414`), porque os casos escrevem no equipamento. Em simulação, o `jk class verify` é local.

### 5.12 Dispositivos e factos do lock

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/devices`, `/v1/devices/{device}` | viewer | | driver (com versão, tier e digest do descritor), barramento, endereço (sem credenciais), perfil, estado do link (`link_state`, os seis valores da ref/00 §6: `connecting`, `up`, `degraded`, `down`, `drift`, `stopped`), latência, timeouts, reconexões, deriva, factos, ações (`Device`) |
| POST | `/v1/devices/{device}/stop` | operator | M | para o driver do dispositivo; `reason` obrigatório (`ReasonRequired` → `Device`) |
| POST | `/v1/devices/{device}/start` | operator | M | volta a arrancar um driver parado com `stop` (`Device`) |
| GET | `/v1/lock/facts` | viewer | | factos da geração em execução com a forma do lock: `[discovered.*]`, `[fallback.*]`, tempo de reinício do `jokerd` medido, e o excerto TOML pronto (`LockFacts`) |
| GET | `/v1/lock/drift` | viewer | | entradas de deriva (`J0401`), com as saídas bloqueadas em `prod` |
| POST | `/v1/lock/probe` | admin | M | recolhe os mesmos factos para dispositivos que o nó **ainda não tem numa geração**, só com leituras (trabalho `lock_probe`): o pedido traz o manifesto efetivo (`manifest`) ou só as secções dos dispositivos (`devices`: nome → a secção `[devices.<d>]` em JSON, com os barramentos que usam), e o que recolher (`what`: `discover`, `fallback`, `hello`) (`ProbeRequest`) |

**`stop` e `start` de um dispositivo** (`jk device stop|start`, documento 07, §4). `stop` fecha o driver do dispositivo como numa paragem ordenada (as escritas pendentes recebem `not-connected` no registo de decisões; o `sysfs` repõe o `trigger` dos LEDs, ref/01 anexo A), põe os sinais em `bad(not_connected)` e o `link_state` em `stopped` (ref/00 §6), e as saídas ficam cobertas pelo fallback do dispositivo (documento 01, §8), como com um driver morto (ref/06 §5.7), mas sem reinício com backoff. Num `joker-link`, o link fecha e o MCU fica com os seus fallbacks locais. Como o `stop` de uma app, é **volátil**: vale até ao próximo arranque do `jokerd` ou à próxima geração que mexa no dispositivo, e aparece no `Device` como `stopped_by` (principal, quando, motivo). `start` faz `connect`, a verificação de deriva e o `resync` de uma religação (ref/02 §3.1.5). Os dois são auditados (§4) e idempotentes (200 num dispositivo já parado ou já a correr); durante uma troca de geração que mexe no dispositivo, `J-E1501` (409). Um dispositivo parado à mão não é uma falha: o evento `joker.device.<d>.link` (ref/06 §2.12) leva `stopped_by` no detalhe, e o alarme `.offline` não se levanta.

O `jk lock --node` usa `lock/probe` quando o manifesto local tem dispositivos que o nó ainda não conhece (o fluxo do documento 07, §9, faz `jk lock` antes do primeiro `jk apply`), e `lock/facts` depois (ref/02 §3.1.3). **O probe nunca escreve nem liga saídas**, nem os registos de fallback: `fallback.configure = true` só atua na ativação de uma geração. `discover` devolve identidade, mapa efetivo e digest; `fallback`, os valores atuais dos registos de fallback; `hello`, o `HELLO` de um MCU `joker-link` (ref/07 §6.4), sem abrir uma sessão `full`.

### 5.13 Gerações e blobs

**Blobs** (armazém endereçado por conteúdo; semântica próxima da OCI Distribution):

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| POST | `/v1/blobs/missing` | admin | | `{digests}` → `{missing}` |
| HEAD, GET | `/v1/blobs/{digest}` | admin | | existe; descarrega (com `Range`) |
| POST | `/v1/blobs/uploads` | admin | M | com `?digest=` e corpo `octet-stream` ≤ 8 MiB: upload monolítico (201). Com `UploadStart` (`digest`, `size`, `kind`): sessão de upload (202 + `Location`). Blob já existente: 201 com `existed: true` |
| GET | `/v1/blobs/uploads/{upload}` | admin | | `received` (próximo offset), para retomar |
| PATCH | `/v1/blobs/uploads/{upload}` | admin | M | bloco com `Content-Range: bytes a-b/total`, ≤ 8 MiB, contíguo (`J-E1605`, 416) |
| PUT | `/v1/blobs/uploads/{upload}?digest=` | admin | M | fecha (com um último bloco opcional) e verifica o SHA-256 (`J-E1603`) |
| DELETE | `/v1/blobs/uploads/{upload}` | admin | M | cancela |

Uma sessão de upload expira 1 h depois do último bloco (`J-E1604`). Os blobs que nenhuma das 10 gerações guardadas usa são apagados.

**Tipos de artefacto** (`kind`): os da tabela única da ref/00 §7.1 (D13), na forma curta: `joker.config.v1+cbor`, `joker.manifest.v1+toml`, `joker.lock.v1+toml`, `joker.provenance.v1+json`, `joker.app.v1+wasm`, `joker.app.v1+cwasm` (AOT opcional; sem ele o nó compila), `joker.driver.v1+wasm`, `joker.driver.v1+tar`, `joker.profile.v1+toml`, `joker.board.v1+toml`, `joker.sim.v1+toml`, `joker.class.v1+jkc`, `joker.class.contract.v1+json`, `joker.class.conformance.v1+toml`, `joker.micro-firmware.v1`, `joker.generation.v1`, `joker.secrets.v1`, `joker.key.v1+pem`, `joker.runtime.v1`, `joker.trace.v1`. `joker.model.v1+jkm` está reservado para a 1.1 (`J-E1017`). Um `kind` fora da tabela é `J-E1012`.

**Gerações:**

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/generations` | viewer | | as gerações guardadas (`Generation`: `gen`, `hash`, estado, mensagem, fonte Git, diff) e o **histórico de ativações** (`activations`: quando, `origin`, principal, geração de onde se saiu, motivo) |
| POST | `/v1/generations` | admin | M | valida e planeia; `dry_run=true` não guarda nada (200); senão prepara uma geração `pending` (201) |
| GET | `/v1/generations/{gen}` | viewer | | uma geração, com plano e diagnósticos; `ETag` |
| DELETE | `/v1/generations/{gen}` | admin | M | descarta uma `pending` (fica `discarded`, `discard_reason = deleted`) |
| GET | `/v1/generations/{gen}/manifest` | viewer | | manifesto efetivo (`application/toml`) |
| GET | `/v1/generations/{gen}/lock` | viewer | | lock |
| GET | `/v1/generations/{gen}/provenance` | viewer | | proveniência de cada chave (ficheiro, linha, operação `set`, `append`, `unset`, `widen`); `key=` filtra |
| POST | `/v1/generations/{gen}/activate` | admin | M | troca; `If-Match` obrigatório (`ActivateRequest`) |
| POST | `/v1/generations/{gen}/confirm` | admin | M | confirma a geração em `confirming` |
| POST | `/v1/generations/rollback` | admin | M | **reativa** uma geração guardada (a anterior, por omissão); `If-Match` obrigatório (`RollbackRequest`) |

`{gen}` aceita o inteiro, o hash, ou `current`, `previous` (a última confirmada antes da atual), `pending`, `confirming`.

**`GenerationProposal`:**

| Campo | Tipo | Obrig. | Regra | Erro |
|---|---|---|---|---|
| `manifest` | `ContentRef` (`digest` de um blob `joker.manifest.v1+toml`, ou `content` ≤ 1 MiB) | sim | o manifesto **efetivo** (overlays já fundidos pela `jk`) | `J-E1602`, J-codes |
| `lock` | `ContentRef` | sim | | idem |
| `provenance` | `ContentRef` | não | mapa chave → origens, produzido pela `jk`; o nó guarda-o e serve-o | — |
| `artifacts` | lista de `ArtifactRef` (`digest`, `kind`, `name`, `size`, `signature`) | sim | `name` identifica o artefacto no projeto: a chave do manifesto para as apps (`apps.peak_shaver`) e, para ficheiros, o caminho relativo à raiz com o sufixo do tipo (ADR 0026: `profiles/local/xy-md02@1.profile.toml`, `boards/rpi5.board.toml`, `keys/site-ops.pub`); um perfil ou uma placa com `name` sem o sufixo do seu `kind` é `J-E1012` | `J-E1602` (lista os que faltam) |
| `config` | `ContentRef` do artefacto `joker.config.v1+cbor`: o COSE_Sign1 cuja carga útil é o descritor da geração (ref/10 §2.6.2) | em `prod` | assinado por uma chave com o papel `config` na política efetiva (ref/02 §2.5.4); o nó reconstrói o descritor a partir de `manifest`, `lock` e `artifacts` e tem de obter os mesmos bytes | `J-E1607` |
| `base` | digest ou `null` | sim (`null` só em `dev`) | geração sobre a qual o plano é feito | `J-E1609` |
| `message` | string ≤ 1 KiB | não | | |
| `source` | `{repo, commit, path, dirty}` | não | | |

**Hash da geração.** É o da ref/10 §2.6.2: o SHA-256 do **descritor** em CBOR determinístico (manifesto efetivo com as omissões materializadas e sem `x_*`, digest canónico do lock, contratos das classes, artefactos por `name`, `media_type` e `digest`, e o `mode`). É o mesmo hash que o agente usa em `generation-target.hash`, que o Joker Micro guarda no `source_hash` do blob, e que o `Joker-Generation` e o `ETag` mostram. A proveniência não entra no hash: não muda o comportamento.

**Origem.** Cada geração criada pela API tem `origin = local` (socket ou TCP) ou `session` (túnel de uma sessão remota), e é recusada se `[policy] apply_from` do nó (ref/02 §3.5.1) não a permitir: `local` precisa de `local`, `session` precisa de `deck` (`J-E1615`).

**Validação no nó.** O nó corre as mesmas regras do `jk check` (o crate `joker-manifest`) com os factos que só ele tem (placa, kernel, deriva, WCET do lock, firmware dos MCUs). Falhas: `J-E1601` (422) com todos os diagnósticos em `errors`, cada um com o seu J-code e `spans` no manifesto efetivo; a `jk` traduz os spans para o ficheiro de origem com a proveniência.

**`Plan`:** `base`, `target`, `requires_confirm` e `confirm_reasons` (`network`, `security`, `uplinks`, `api`), `disruption` (`none`, `hot`, `partial`, `full`), `changes` (cada uma com `kind`, `target`, `effect` `none` \| `hot` \| `reload` \| `restart`, `detail` e campos de/para), `signal_ids` (mantidos, novos, removidos, renumerados), `admission` (utilização por CPU `rt` face ao limite, M23), `micros` (OTA de firmware ou de geração que a troca inclui).

**Estados e transições** (`generation_state`, ref/00, §6):

```
POST /generations ──▶ pending ──activate──▶ confirming ──confirm──▶ confirmed
                        │   (sem janela) ─────────────────────────▶ confirmed
                        │                       │
                        │                       ├─ janela expirou ──▶ rolled_back  (a base é reativada)
                        │                       ├─ saúde falhou ────▶ rolled_back
                        │                       └─ rollback ────────▶ rolled_back
                        ├─ ativação falhou ──────────────────────────▶ failed (a base continua)
                        └─ descartada / 1 h sem ativar ──────────────▶ discarded (deleted / expired)

 rollback de uma geração confirmada G: reativa uma guardada F (mesmo gen, mesmo hash);
 G passa a rolled_back, F volta a confirmed (ou a confirming, se se pedir janela)
```

- **Uma transição de cada vez.** No máximo uma geração `pending` e uma `confirming` por nó. Um `POST` com uma `pending` de outro principal, ou durante uma troca, é `J-E1608` (409). Uma `pending` do mesmo principal é substituída (a anterior fica `discarded`, `discard_reason = superseded`). O agente passa pelo **mesmo trinco** (ref/10 §3.4.2): uma troca do agente em curso dá `J-E1608` a um `POST` local, e uma transição local em curso faz o agente esperar.
- **`activate`:** `If-Match` = `base`. `confirm_window` entre 30 s e 24 h (`J-E1623`); obrigatória em `prod` quando `plan.requires_confirm` (ADR 0015; `J-E1610`). A troca acontece num limite de tick de cada grupo afetado (documento 01, §15). Com `wait` (omissão), a resposta chega com o estado `confirming`, `confirmed` ou `failed`.
- **Saúde durante a janela:** a geração volta atrás se, antes da confirmação, um grupo entrar em trip por overrun, uma app nova entrar em `crashloop`, ou um driver novo não chegar a `up` dentro da janela. O motivo fica em `failure`.
- **`confirm`** fora de `confirming` é `J-E1611` (409); se a geração já voltou atrás, o erro diz `rolled_back` e a `jk` sai com o código 5.
- **`confirm` e os MCUs.** Quando a ativação levou um blob de geração (ou um firmware) a um MCU atrás de um `joker-link` e o pôs em experiência, o `confirm` da geração do nó confirma **também** esse OTA pendente: o H envia `OTA confirm` a cada MCU em experiência dessa ativação (ref/07 §9.4), e a resposta só chega `confirmed` depois de todos responderem `confirmed`. Um MCU que já tenha voltado atrás sozinho (`rolled_back`, `J-E2409`) faz a resposta falhar com esse código, e a geração do nó continua `confirming`: quem confirma decide se volta atrás. Sem janela de confirmação no nó (uma ativação sem `confirm_window`), o H confirma o MCU sozinho pela saúde, como na ref/07 §9.4.
- **`rollback`** (D9): `to` (omissão: `previous`). **Reativa** a geração guardada com o mesmo `gen` e o mesmo `hash`: não cria uma geração nova. O histórico ganha uma **ativação** com `origin = rollback`, o principal e o motivo, e a geração de onde se saiu fica `rolled_back`. Uma geração cujos blobs já foram apagados é `J-E1613`. Voltar a uma geração que já esteve confirmada não exige janela; pode pedir-se uma.
- **`dev`:** `jk dev --node` usa o mesmo caminho, com `base: null`, sem assinatura e sem janela. É rápido porque o diff é mínimo (documento 01, §15), e não porque haja um atalho.
- **Origem de cada ativação** (`origin`, o `gen-origin` da ref/10 §2.2): `local`, `session`, `agent`, `import`, `shadow`, `rollback`, `boot`. As gerações criadas pelo nó (sombra, promoção automática) são `shadow`; o arranque que volta à geração confirmada é `boot`.

### 5.14 Apps e sombra

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/apps`, `/v1/apps/{app}` | viewer | | versão, digest, mundo WIT, SDK, estado (o `app_state` da ref/00 §6: `starting`, `running`, `crashloop`, `stopped`, `failed`, `shadow`, `updating`), instâncias de ciclo e async (estado, grupo, memória, tarefas com WCET e overruns), escritor, traps, reinícios, backoff, sombra, instâncias implementadas, `meta` (`joker:meta` em JSON) (`App`) |
| POST | `/v1/apps/{app}/start` | operator | M | arranca uma app parada |
| POST | `/v1/apps/{app}/stop` | operator | M | para; `reason` obrigatório |
| POST | `/v1/apps/{app}/restart` | operator | M | recria as duas instâncias; tira de `crashloop` |
| GET | `/v1/apps/{app}/kv` | admin | | lista chaves (`prefix`) ou lê uma (`key`; valor em base64); **não há escrita** |
| GET | `/v1/shadows` | viewer | | sombras ativas e recentes |
| POST | `/v1/shadows` | admin | M | `ShadowStartRequest`: `target` (`app:<nome>`), `candidate` (artefacto), `until`, `promote`, `reason`; `If-Match` |
| GET | `/v1/shadows/apps/{app}` | viewer | | estado, divergência por saída (p50, p95, máx.), critérios e se passam (`Shadow`) |
| GET | `/v1/shadows/apps/{app}/diff` | viewer | | séries `shadowed` contra `applied` (`ShadowDiff`) |
| POST | `/v1/shadows/apps/{app}/promote` | admin | M | promove tudo ou só `outputs` (promoção parcial, documento 08, rodada 6); `force` sem os critérios; `confirm_window` |
| POST | `/v1/shadows/apps/{app}/abort` | admin | M | aborta |

- **`stop` é volátil:** vale até ao próximo arranque do `jokerd` ou à próxima geração, e aparece no estado como `stopped_by` (principal, quando, motivo). Parar uma app de forma duradoura é mudar a geração. As saídas da app seguem o lease e vão para `safe` sozinhas (documento 01, §8).
- Operações de ciclo de vida redundantes (`start` numa app a correr) são idempotentes (200). Durante uma troca de geração que mexe na app, `J-E1501` (409).
- **A sombra passa por gerações.** `POST /v1/shadows` cria uma **geração derivada** da atual com a secção `[apps.<a>.shadow]` (documento 03, §8) e o candidato como artefacto, e ativa-a pelo caminho normal (ativação com `origin = shadow`, `derived_from`). Promover e abortar são outras gerações derivadas. Uma promoção automática (critérios cumpridos, `promote: true`) também é uma geração nova, criada pelo nó. Assim a sombra aparece no histórico de gerações, sobrevive a reinícios e volta atrás como tudo o resto; o custo é o manifesto do nó divergir do Git, e o `jk diff` mostra-o.
- Uma sombra já ativa para a app é `J-E1511`; promover sem os critérios nem `force` é `J-E1512`.
- A sombra de **modelos** fica em `/v1/shadows/models/{model}`, reservado para a 1.1.

### 5.15 Runtime

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/runtime` | viewer | | versão, slot ativo, slots A/B (versão, digest, instalação, saúde), atualização em curso (`RuntimeState`) |
| GET | `/v1/runtime/drivers` | viewer | | o **manifesto de drivers** embutido no binário (ref/06 §3.8): por driver, nome, versão, tier, protocolos, capacidades e digest do descritor; e os tier W e X carregados pela geração (`RuntimeDriver`). É o que o relatório ao Deck leva em `drivers` (ref/10 §2.10.1) |
| POST | `/v1/runtime/upgrades` | admin | M | `RuntimeUpgradeRequest`: `artifact` (blob `joker.runtime.v1` já carregado), `version`, `signature`, `health_window`, `allow_downgrade`, `reason` → trabalho `runtime_upgrade` |
| POST | `/v1/runtime/rollback` | admin | M | volta ao outro slot → trabalho `runtime_rollback` |

O trabalho instala no slot inativo, troca e reinicia o `jokerd`: a ligação cai. A `jk` reconhece o reinício pelo `boot_id` novo em `/v1/info`, e acompanha o trabalho até `succeeded` (saúde passou na `health_window`) ou `failed` (voltou ao slot anterior; documento 01, §16). Artefacto não assinado em `prod`, de outra arquitetura ou mais antigo sem `allow_downgrade`: `J-E1620`. Uma atualização em curso: `J-E1621`. O nó não descarrega a runtime de um registo: a `jk` carrega-a (local-first); o Deck usa o seu próprio caminho (ref/10).

### 5.16 Traços

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/traces`, `/v1/traces/{trace}` | viewer | | traços no nó (`Trace`) |
| POST | `/v1/traces` | operator | M | cria (`TraceRequest`) |
| POST | `/v1/traces/{trace}/stop` | operator | M | termina uma gravação antes do fim; o ficheiro fica válido |
| GET | `/v1/traces/{trace}/file` | viewer | | descarrega o `.jktrace` (`Range`); antes de `ready` ou `stopped`, `J-E1706` (409) |
| DELETE | `/v1/traces/{trace}` | operator | M | apaga |

| `source` | Campos | Comando | Regra |
|---|---|---|---|
| `record` | `duration`, `groups` (omissão: todos), `async` | `jk record --for 2h --groups fast` | grava daqui para a frente; `async = true` grava também as leituras async (documento 09, ataque 10). Começa pelo segmento de estado: o `export-state` de cada app dos grupos gravados, entre dois ticks (documento 16, §7), de onde o `jk replay` parte |
| `ring` | um `groups`, `last` ou `from`/`to` | `jk trace-dump --group fast --last 30m` | extrai do anel `trace.continuous` do grupo (exato); sem anel, `J-E1701`; fora da cobertura, `J-E1702`. Sem segmento de estado: o replay começa do `init` |
| `history` | `from`, `to`, `app` | `jk trace-from-history "03:00..03:30" --app peak_shaver` | reconstrói das séries do historian; `approximate: true` e avisos sobre as entradas que não estavam gravadas ao ritmo do grupo |
| `micro` | `device` | — | pede o anel do MCU por `TRACE` (documento 19, U9); o nó comprime os segmentos |

- Descarregar um traço que ainda não está `ready` ou `stopped` é `J-E1706` (409).
- Os traços contam para uma quota de disco (configuração do nó, ref/02); cheia, `J-E1703` (507).
- O **replay** corre no portátil (`jk replay`): não há endpoint de replay.

### 5.17 Trabalhos e diagnóstico

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/jobs`, `/v1/jobs/{job}` | viewer | | trabalhos (`Job`: `kind`, `state`, progresso, `result`, `error`) |
| POST | `/v1/jobs/{job}/cancel` | o papel que o criou | M | cancela |
| GET | `/v1/doctor` | viewer | | último relatório guardado |
| POST | `/v1/doctor` | admin | M | `DoctorRequest`: `checks` (`base` sempre, `rt`, `ml`, `network`), `rt_duration` (omissão 60 s), `allow_live` → trabalho `doctor` |
| POST | `/v1/discovery` | admin | M | `DiscoveryRequest`: `uri`, `unit`, `rate` (omissão 10/s), `timeout` → trabalho `discovery` |
| POST | `/v1/profile-tests` | admin | M | `ProfileTestRequest`: `profile`, `uri` (gramática da ref/03 §2.2.20), `unit` (equivale ao `?unit=` da URI; os dois, diferentes, `J-E1708`), `timeout` → trabalho `profile_test`: só leituras; quando a porta ou o endereço já é um barramento da geração ativa, os pedidos entram no escalonador desse barramento, e não competem com o driver |
| POST | `/v1/bench` | admin | M | `BenchRequest`: `manifest`, `apps`, `tasks`, `iterations`, `allow_live` → trabalho `bench` |

- **`doctor`:** verificações do documento 07, §3, e do documento 09, ataque 5 (ids em `DoctorReport.checks`: `kernel.preempt_rt`, `cpu.isolcpus`, `cpu.governor`, `irq.affinity`, `watchdog`, `clock`, `disk`, `permissions`, `groups`, `accelerators`, `device_fallbacks`, `restart_time`, `rt.major_faults`, `rt.hwlat`, `rt.cyclictest`, `files.drift`, `fieldbus.concurrent_clients`). `rt` com saídas ativas em `prod` exige `allow_live` (`J-E1707`): o `hwlatdetect` e o `cyclictest` perturbam o núcleo `rt`. `network` é da 1.3 (modo gateway) e na 1.0 dá `skip`.
- **Descoberta:** só leituras, com limite de ritmo (documento 05, §7). Esquema desconhecido: `J-E1708`. O resultado lista o que encontrou (identidade, modelos SunSpec, perfil sugerido, digest do mapa); fixá-lo é o `jk lock`.
- **`bench`:** mede o WCET das tarefas de ciclo no alvo com imagens de entrada sintetizadas; **as propostas são descartadas**, nada chega ao árbitro. Com uma geração em `prod` e saídas ativas, exige `allow_live`, corre à prioridade `SCHED_FIFO` mais baixa e marca os resultados com interferência. O resultado vem com o excerto `[wcet]` do lock.
- Um trabalho do mesmo tipo em curso: `J-E1705` (409).

### 5.18 Uplinks, pontes, MCUs

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/uplinks`, `/v1/uplinks/{uplink}` | viewer | | estado, endpoint sem credenciais, lag do cursor, backlog, orçamento (limite, usado, projeção, classes degradadas; M12), perdas (`Uplink`) |
| GET | `/v1/bridges` | viewer | | pontes: estado, clientes, escritor, `writable`, escritas aceites, rejeitadas e rejeitadas por anti-replay (`Bridge`) |
| GET | `/v1/micros`, `/v1/micros/{device}` | viewer | | MCUs por Joker Link (`Micro`): transporte, `link_state`, **modo da sessão** (`session_mode`, ref/00 §6) e o motivo quando não é `full`, **versão negociada** (`link_version`), autenticação, placa, firmware (versão, slot, digest da imagem, **descritor de firmware** e o seu digest, hash das capacidades, `security_version`), ABI, geração (`gen_seq`, hash, `embedded` \| `flash`, **estado de confirmação** e tempo que falta na janela), registo, catálogo, classes, drivers (o resumo da chave 15), relógio, **parâmetros alterados localmente**, o **último `HELLO`** (descodificado), **contadores** (tramas, CRC-32C, autenticação, replay, retransmissões, ressincronizações; os do `jk_link_status`, ref/08 §2.6.p) e RTT, OTA em curso |
| POST | `/v1/micros/{device}/ota` | admin | M | `MicroOtaRequest`: `kind` (`firmware`, `generation`), `artifact`, `signature` (o COSE_Sign1 do manifesto de OTA, ref/07 §9.1; numa geração já vem no *trailer* do blob e pode omitir-se), `confirm_window` (a `confirm_ms` do `activate`), `reason` → trabalho `micro_ota`: `begin`, transferência, teste, confirmação ou reversão (ref/07 §9) |

- OTA de um MCU desligado: `J-E1802` (503). Artefacto incompatível (placa, ABI, contador de segurança): `J-E1803`. OTA em curso: `J-E1804`. Um dispositivo que não é `joker-link`: `J-E1801`.
- O OTA de MCUs que faz parte de uma geração (o plano lista-o em `micros`) é orquestrado pela ativação; este endpoint é para o caso manual.

### 5.19 Simulação (`mode = "dev"`)

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/sim` | viewer | | dispositivos `sim`, descritores de painel (sinal, `lamp`, `button`, `switch`, `slider`, `value`, tecla, gama), entradas forçadas |
| PUT | `/v1/sim/inputs/{path}` | operator | M | força o valor de um sinal de um dispositivo `sim` (qualidade `Good(overridden)`, documento 02, §4); `ttl` opcional |
| DELETE | `/v1/sim/inputs/{path}` | operator | M | liberta; o modelo de simulação volta a mandar |

Em `mode = "prod"`, `J-E1018` (409). Um sinal que não é de um dispositivo `sim`: `J-E1930`. É o que o `jk dev --panel` usa para os botões e sliders quando corre contra um nó (`--node`).

### 5.20 Reservados (1.1+)

`/v1/models`, `/v1/models/{model}`, `/v1/models/{model}/bench`, `/v1/training`, `/v1/shadows/models/{model}` (e `promote`, `abort`), `/v1/signals/{path}/blocks`, `/v1/streams`. Na 1.0 respondem 501 com `J-E1017`. Estão no OpenAPI com `x-joker-since: '1.1'`.

### 5.21 Agente e confiança do nó

O agente (ref/10) e a confiança do nó também se leem e comandam pela API local: é por aqui que a `jk agent …`, a `jk node …` e a `jk session …` falam com o nó (ref/10 §2.18).

| Método | Caminho | Papel | M | Faz |
|---|---|---|---|---|
| GET | `/v1/agent` | viewer | | estado do agente (`jk agent status`): ligado, autoridade (`deck_id`), transporte, último contacto, `desired_seq`, `report_seq`, `event_seq`, fila (eventos, bytes, perdidos, o mais antigo), pausa, `hold`, sessões remotas abertas (`AgentState`) |
| GET | `/v1/agent/desired` | viewer | | o DSD atual descodificado, em JSON (a vista do ref/10 §2.6.7), com o estado da aplicação (`apply-state`, fase, espera, código, causas, bytes descarregados) (`AgentDesired`) |
| POST | `/v1/agent/pause` | admin | M | suspende as trocas (`ttl` ≤ 7 d, `reason`); os downloads, os relatórios e os rollbacks continuam (ref/10 §3.7.2) |
| POST | `/v1/agent/resume` | admin | M | retoma; também termina um `hold` de alterações locais (ref/10 §3.7.1) |
| POST | `/v1/agent/import` | admin | M | aplica um DSD trazido à mão (`jk agent import`, ref/10 §3.7.3): `desired` (o COSE em base64) e os digests dos artefactos já carregados por `/v1/blobs/uploads` → trabalho `agent_import` |
| GET | `/v1/agent/sessions` | operator | | sessões remotas do Deck: abertas e à espera de consentimento (**1.3**) |
| POST | `/v1/agent/sessions/{session}/accept` | operator | M | dá o consentimento local a uma sessão (`consent = "local"`, ref/10 §3.10) (**1.3**) |
| DELETE | `/v1/agent/sessions/{session}` | operator | M | fecha uma sessão remota (**1.3**) |
| GET | `/v1/node/trust` | viewer | | âncoras, bundle ativo (versão, autoridade, chaves e papéis, `deck_urls`, pinos, `min_mode`), pisos por papel, revogadas (`TrustState`) |
| PUT | `/v1/node/trust/bundle` | admin | M | instala um bundle de confiança (`application/cose`): assinado por uma âncora e com `version` maior (ref/02 §2.5.4; `J0918`, `J0939`) (`jk node trust push`) |
| PUT | `/v1/node/trust/anchors` | admin | M | troca as âncoras (`jk node trust set`): um documento COSE assinado por uma âncora atual, com as âncoras novas e as revogadas (ref/02 §3.2.3); o `jokerd` reescreve o `node.toml` |

- Nenhum destes endpoints serve numa sessão remota, exceto `GET /v1/agent` e `GET /v1/agent/desired` (§2.3, `J0986`).
- `pause`, `resume` e `import` com o agente desligado (sem autoridade) são `J-E1940`. Os endpoints de sessões remotas respondem `J-E1017` antes da 1.3.
- As sessões remotas (`/v1/agent/sessions`) são outra coisa que as ligações da própria API (`/v1/sessions`, §5.3).

---

## 6. WebSocket e Server-Sent Events

### 6.1 Ligação

- `GET /v1/ws` com `Upgrade: websocket` e `Sec-WebSocket-Protocol: joker.v1`. Outro subprotocolo, ou nenhum: 400 `J-E1900`.
- A autenticação e o papel são os da ligação HTTP (§2). Um `Origin` presente é recusado (§2.4).
- Mensagens de texto, um objeto JSON por mensagem, com o campo `op`. Mensagens binárias: fecho 4000.
- **Não há escritas por WebSocket na 1.0.** As escritas vão por HTTP, onde têm anti-replay, idempotência, auditoria e códigos de erro uniformes. O WebSocket é só para ver.

### 6.2 Mensagens do cliente

| `op` | Campos | Resposta |
|---|---|---|
| `subscribe` | `id`, `topic`, filtros (§6.4), `min_interval`, `mode`, `initial`, `mono`, `fields`, `buffer` | `subscribed` ou `error` |
| `unsubscribe` | `id`, `sub` | `unsubscribed` |
| `resume` | `id`, `session`, `subs: [{sub, last_seq}]` | `resumed` (§6.7) |
| `ping` | `id` | `pong` |
| `list` | `id` | `subscriptions` |

Um `op` desconhecido é `J-E1901`; um tópico desconhecido, `J-E1902`; uma mensagem mal formada, `J-E1900`. Estes erros chegam como `error` e **não fecham a ligação**, exceto se se repetirem mais de 10 vezes num minuto (fecho 4000).

### 6.3 Mensagens do servidor

| `op` | Quando | Campos |
|---|---|---|
| `welcome` | logo a seguir ao upgrade | `session`, `boot_id`, `api`, `role`, `generation`, `heartbeat` (`15s`), `resume_window` (`60s`), `limits` |
| `subscribed` | subscrição aceite | `id`, `sub`, `topic`, `resolved` (sinais resolvidos), `min_interval` efetivo |
| `data` | dados | `sub`, `seq`, `items` |
| `resync` | estado completo (inicial, depois de um `gap`, depois de um `resume` sem réplica) | `sub`, `seq`, `items` |
| `gap` | itens perdidos | `sub`, `lost`, `reason` (`overflow`, `intake_overflow`, `resume_unavailable`, `generation`, `role_changed`), `ts` |
| `resolved` | a geração mudou e o seletor resolve para outra lista | `sub`, `generation`, `added`, `removed` |
| `heartbeat` | 15 s sem outra mensagem | `ts` |
| `pong`, `unsubscribed`, `resumed`, `subscriptions` | respostas | |
| `error` | erro de um pedido ou de uma subscrição | `id`, `sub`, `problem` (RFC 9457) |
| `closing` | antes de o servidor fechar | `code`, `problem` |

**`seq`** conta as mensagens `data` e `resync` de cada subscrição, a começar em 1. É o que o cliente guarda para retomar.

### 6.4 Tópicos

| Tópico | Tipo | Itens | Filtros | Papel |
|---|---|---|---|---|
| `signals` | estado | `Sample` | `sel`, `mode`, `fields`, `mono` | viewer |
| `outputs` | estado | `OutputState` resumido (efetivo, aplicado, nível ativo, último resultado, trip) | `sel` | viewer |
| `status` | estado | `Status` | — | viewer |
| `metrics` | estado | `Metrics` | — | viewer |
| `signal_trace` | estado | um registo por tick com alterações no sinal: amostra adquirida, leitores, propostas, árbitro, envelope, commit, readback; `skipped_ticks` | `path` | viewer |
| `alarms` | eventos | `AlarmEvent` | `sel`, `severity`, `states` | viewer |
| `events` | eventos | `EventRecord` | `sel` | viewer |
| `decisions` | eventos | `Decision` | `sel` (saídas), `outcome`, `writer` | viewer |
| `logs` | eventos | `LogEntry` | `source`, `app`, `level` | viewer |
| `manual` | eventos | `ManualOverride` com `event` (`set`, `renewed`, `expiring`, `expired`, `released`, `taken_over`, `restored`) | `sel` | viewer |
| `intents` | eventos | `Intent` | `states` | viewer |
| `jobs` | eventos | `Job` | `job` | viewer |
| `invocations` | eventos | `Invocation` | `invocation` | viewer |
| `generations` | eventos | `Generation` | — | viewer |
| `audit` | eventos | `AuditEntry` | — | admin |
| `frames`, `blocks` | — | reservados para a 1.1 | — | — |

### 6.5 Taxa e coalescência

- **Tópicos de estado** (`signals`, `outputs`, `status`, `metrics`, `signal_trace`) são **amostrados**, não enfileirados. A cada `min_interval`, o servidor lê o estado atual e envia o que mudou (`mode = change`, omissão: só sinais com `seq` novo) ou tudo (`mode = interval`). Entre dois envios, as amostras intermédias perdem-se de propósito: é a mesma política *latest-wins* das caixas de saída (documento 01, §3.4). O salto no `seq` de cada sinal diz ao cliente quantas amostras foram coalescidas.
- `min_interval`: omissão `100ms` em `signals`, `outputs` e `signal_trace`; `1s` em `status` e `metrics`. Mínimo `10ms` (`J-E1905`); em `outputs`, o mínimo é o período do grupo dono; em `status` e `metrics`, `1s`.
- **Tópicos de eventos** não se coalescem: cada item é entregue, ou a sua perda é declarada com um `gap`.
- `initial: true` (omissão): a primeira mensagem é um `resync` com o estado atual (tópicos de estado) ou os alarmes ativos (`alarms`).

### 6.6 Contrapressão: um cliente lento nunca trava o núcleo

```
 núcleo (rt, io)                     svc (tokio, bg)                                 rede
 ───────────────                     ───────────────                                 ────
 slots (seqlock) ◀── leitura sem espera ── amostrador por subscrição ──▶ fila de saída da ligação ──▶ socket
 filas SPSC limitadas ──▶ intake de eventos ──▶ anel por subscrição (buffer) ──┘      (1 MiB)
 (alarmes, decisões, eventos, logs)       │
                                          └─ intake cheio → perda contada (joker.api.dropped) → gap intake_overflow
```

1. **O núcleo não conhece clientes.** Os tópicos de estado leem os slots com o protocolo seqlock (documento 01, §3.2), sem espera e sem escrever nada que o núcleo leia. Os tópicos de eventos consomem as filas lock-free limitadas que já existem para o historian e os uplinks (ADR 0013). Nenhuma ação de um cliente chega a uma thread `rt`.
2. **Estado:** um cliente lento só recebe menos vezes. O amostrador não gera a mensagem seguinte enquanto a anterior não sair da fila da ligação, e a taxa efetiva baixa sozinha.
3. **Eventos:** cada subscrição tem um anel de `buffer` itens (64–4 096, omissão 1 024). Cheio, saem os mais antigos e o cliente recebe um `gap` com `lost`. Em `alarms`, a seguir ao `gap` vem um `resync` com a tabela atual de alarmes, para o estado nunca ficar errado.
4. **Ligação:** se a fila de saída (1 MiB) ficar cheia durante mais de **30 s**, o servidor envia `closing` e fecha com **4008** (`J-E1906`). Um cliente que não lê não ocupa memória para sempre.
5. **Orçamento do nó:** a soma das taxas de amostragem de todas as subscrições está limitada a 1 000 000 amostras/s. Uma subscrição que o ultrapasse é recusada (`J-E1903`), com a taxa que ainda cabe no `detail`.

### 6.7 Retoma depois de uma reconexão

- O servidor guarda o estado de uma sessão (subscrições, anéis de eventos e as últimas 256 mensagens enviadas de cada subscrição, até 1 MiB) durante `resume_window` (60 s) depois de a ligação cair.
- O cliente liga-se de novo e envia `resume` com a `session` antiga e o último `seq` de cada subscrição. Para cada uma, o servidor responde em `resumed` com `replayed` (reenvia as mensagens depois de `last_seq`) ou `resync` (manda o estado atual e, nos tópicos de eventos, um `gap` com `resume_unavailable`).
- Fora da janela, ou depois de um reinício do `jokerd` (`boot_id` diferente), a sessão já não existe: `J-E1907`, e o cliente subscreve de novo. Para eventos antigos, o caminho é o REST com intervalo (`/v1/alarm-events`, `/v1/decisions`, `/v1/events`), que lê do historian.
- A retoma exige o mesmo principal. Outro principal com a mesma `session` é `J-E1907`.

### 6.8 Batimentos

- O servidor envia uma trama WebSocket `ping` a cada 15 s e uma mensagem `heartbeat` quando passaram 15 s sem outra mensagem (para os clientes JavaScript, que não veem as tramas de controlo).
- O cliente tem de dar sinal (trama `pong`, que os navegadores enviam sozinhos, ou qualquer mensagem) a cada 45 s. Senão, fecho **4002** (`J-E1908`).

### 6.9 Troca de geração e papel

- Numa troca de geração, cada subscrição com seletores é resolvida de novo. Se a lista mudar, chega um `resolved` com `added` e `removed`; os caminhos que sobrevivem continuam sem interrupção. Nunca se envia um `SignalId`.
- O papel é recalculado. Se descer, as subscrições que exigiam o papel antigo terminam com `gap` (`role_changed`) e `error`; a ligação continua.

### 6.10 Códigos de fecho

| Código | Significado |
|---|---|
| 1000 | normal |
| 1001 | o `jokerd` vai parar ou reiniciar |
| 1008 | política (papel revogado, origem) |
| 1009 | mensagem do cliente acima de 64 KiB |
| 4000 | erro de protocolo |
| 4002 | batimento em falta (`J-E1908`) |
| 4008 | cliente lento (`J-E1906`) |
| 4009 | sessão fechada por um `admin` |
| 4029 | ligações a mais (`J-E1014`) |

### 6.11 Server-Sent Events

**Decisão: há SSE**, em `GET /v1/stream`, como espelho só de leitura de **uma** subscrição por pedido: `topic`, `sel`, `min_interval`, `initial`, `mono`, e os restantes filtros em `filter` (JSON com os campos do `subscribe`). Serve os scripts e o `curl`, que não falam WebSocket com facilidade, e usa o mesmo motor de subscrições, com a mesma coalescência, os mesmos anéis e o mesmo corte por lentidão.

```
event: subscribed
data: {"op":"subscribed","sub":"s1","topic":"alarms","resolved":14}

event: data
id: 01J8Z3K6QW4R5T6Y7U8I9O0PAS.42
data: {"op":"data","sub":"s1","seq":42,"items":[…]}

: hb
```

- `id` é `<session>.<seq>`. O navegador reenvia-o em `Last-Event-ID` ao voltar a ligar, e isso é um `resume` implícito com as regras do §6.7.
- Batimento: a linha de comentário `: hb` a cada 15 s.
- O SSE conta para o limite de ligações e para o orçamento de amostragem como uma ligação WebSocket.

---

## 7. Códigos novos (J-E1000–J-E1999)

Todos com gravidade `error`, exceto onde se diz. Entram no `codes.toml` (documento 16, §9).

| Código | HTTP | Mensagem |
|---|---|---|
| `J-E1000` | 500 | erro interno da API |
| `J-E1001` | 401 | sem identidade (túnel sem sessão válida) |
| `J-E1002` | 403 | papel insuficiente para esta operação |
| `J-E1003` | 403 | identidade recusada (CN fora da gramática, certificado sem regra aplicável ao transporte) |
| `J-E1004` | 403 | pedido com `Origin`: CORS desligado |
| `J-E1005` | 404 | endpoint inexistente |
| `J-E1006` | 405 | método não permitido |
| `J-E1007` | 406 | tipo de resposta não disponível |
| `J-E1008` | 415 | tipo de conteúdo do pedido não aceite |
| `J-E1009` | 413 | pedido acima do limite (corpo, URL, cabeçalhos, conteúdo em linha) |
| `J-E1010` | 400 | JSON mal formado |
| `J-E1011` | 400 | campo ou parâmetro desconhecido |
| `J-E1012` | 400 | valor de campo ou parâmetro inválido |
| `J-E1013` | 400 | campo obrigatório em falta |
| `J-E1014` | 429 | limite de taxa ou de ligações excedido |
| `J-E1015` | 503 | API indisponível (arranque, paragem) |
| `J-E1016` | 504 | tempo esgotado à espera do núcleo ou do equipamento |
| `J-E1017` | 501 | funcionalidade reservada para uma versão posterior ou fora deste build |
| `J-E1018` | 409 | operação não permitida neste modo (`dev`/`prod`) |
| `J-E1020` | 400 | `Joker-Nonce` ou `Joker-Timestamp` em falta num pedido mutante por TCP |
| `J-E1021` | 400 | `Joker-Timestamp` fora da janela de ±30 s |
| `J-E1022` | 409 | nonce repetido (replay) |
| `J-E1023` | 400 | `Idempotency-Key` obrigatório em falta |
| `J-E1024` | 422 | `Idempotency-Key` reutilizado com outro conteúdo |
| `J-E1025` | 400 | `Idempotency-Key` inválido |
| `J-E1026` | 412 | a geração mudou (`If-Match`) |
| `J-E1027` | 428 | `If-Match` obrigatório em falta |
| `J-E1028` | 409 | pedido com a mesma chave ainda em curso |
| `J-E1029` | 503 | memória de nonces cheia; tentar mais tarde |
| `J-E1030` | 400 | seletor mal formado ou seletores a mais |
| `J-E1031` | 422 | seletor resolve para sinais a mais |
| `J-E1032` | 400 | cursor inválido ou expirado |
| `J-E1033` | 404 | sinal desconhecido (com sugestões) |
| `J-E1034` | 400 | intervalo de tempo inválido |
| `J-E1035` | 400 | literal de duração inválido |
| `J-E1036` | 400 | valor incompatível com o tipo do sinal |
| `J-E1037` | 422 | leitura coerente com sinais a mais |
| `J-E1038` | 422 | resultado do histórico demasiado grande: agregar ou paginar |
| `J-E1039` | 400 | passo, tier ou agregado não aplicável |
| `J-E1100` | 409 | o sinal não é uma saída |
| `J-E1101` | 400 | escrita manual sem TTL (A10) |
| `J-E1102` | 422 | TTL acima de `max_manual` |
| `J-E1103` | 400 | TTL abaixo de 1 s |
| `J-E1104` | 422 | nível fora da faixa `manual` |
| `J-E1105` | 409 | nível manual ocupado por outro principal (usar `takeover`) |
| `J-E1106` | 400 | escrita sem motivo |
| `J-E1107` | 404 | nível manual não ocupado |
| `J-E1108` | 409 | libertar o nível de outro principal exige `force` |
| `J-E1110` | 404 | bilhete de escrita inexistente ou expirado |
| `J-E1120` | 409 | a saída não está em trip |
| `J-E1200` | 404 | alarme desconhecido |
| `J-E1201` | 400 | arquivamento sem TTL |
| `J-E1202` | 422 | TTL de arquivamento acima de `max_shelve` |
| `J-E1203` | 409 | alarme não arquivado |
| `J-E1204` | 409 | nada para reconhecer (`normal` ou `unknown`) |
| `J-E1300` | 404 | intenção desconhecida |
| `J-E1301` | 409 | intenção já decidida ou expirada |
| `J-E1302` | 422 | a aprovação não pode alongar o TTL da intenção |
| `J-E1400` | 404 | ação desconhecida |
| `J-E1401` | 400 | argumentos da ação inválidos |
| `J-E1402` | 409 | permissivo da ação falso |
| `J-E1403` | 503 | fila de ações do dispositivo cheia |
| `J-E1404` | 404 | invocação desconhecida |
| `J-E1405` | 409 | a invocação não pode ser cancelada |
| `J-E1406` | — | (resultado) a ação excedeu o timeout |
| `J-E1407` | — | (resultado) o equipamento devolveu erro |
| `J-E1408` | — | (resultado) a implementação recusou a ação |
| `J-E1409` | — | (resultado) ação cancelada |
| `J-E1410` | 404 | instância desconhecida |
| `J-E1411` | 404 | método desconhecido |
| `J-E1412` | 400 | argumentos do método inválidos |
| `J-E1413` | 404 | classe desconhecida nesta geração |
| `J-E1414` | 409 | conformidade contra um equipamento em `prod` exige `allow_prod` |
| `J-E1415` | — | (resultado) o dispositivo da ação não está ligado |
| `J-E1500` | 404 | app desconhecida |
| `J-E1501` | 409 | ciclo de vida da app ou do dispositivo não permitido agora (troca de geração em curso) |
| `J-E1502` | 404 | chave de `kv` inexistente |
| `J-E1510` | 404 | sombra inexistente |
| `J-E1511` | 409 | já há uma sombra ativa para a app |
| `J-E1512` | 409 | critérios de promoção não cumpridos (usar `force`) |
| `J-E1600` | 404 | geração desconhecida |
| `J-E1601` | 422 | a geração não passou a validação (diagnósticos em `errors`) |
| `J-E1602` | 422 | blobs em falta (lista em `detail`) |
| `J-E1603` | 400 | digest do blob não corresponde ao conteúdo |
| `J-E1604` | 404 | upload inexistente ou expirado |
| `J-E1605` | 416 | `Content-Range` inválido ou fora de ordem |
| `J-E1606` | 413 | bloco ou blob acima do limite |
| `J-E1607` | 422 | assinatura em falta ou recusada pela política de confiança |
| `J-E1608` | 409 | outra transição de geração em curso |
| `J-E1609` | 409 | `base` não é a geração atual |
| `J-E1610` | 422 | a geração exige janela de confirmação (ADR 0015) |
| `J-E1611` | 409 | nada para confirmar |
| `J-E1612` | 409 | transição de estado inválida para esta geração |
| `J-E1613` | 409 | geração de destino já não está guardada |
| `J-E1614` | 410 | geração pendente descartada ou expirada |
| `J-E1615` | 403 | a origem da geração não é permitida pela `[policy] apply_from` do nó |
| `J-E1620` | 422 | artefacto da runtime inválido (assinatura, arquitetura, versão) |
| `J-E1621` | 409 | atualização da runtime em curso |
| `J-E1622` | 409 | o outro slot não tem uma runtime utilizável |
| `J-E1623` | 400 | janela de confirmação fora de 30 s–24 h |
| `J-E1700` | 404 | traço desconhecido |
| `J-E1701` | 409 | o grupo não tem anel de traço contínuo |
| `J-E1702` | 422 | intervalo fora do anel ou da retenção do historian |
| `J-E1703` | 507 | quota de traços esgotada |
| `J-E1704` | 404 | trabalho desconhecido |
| `J-E1705` | 409 | trabalho do mesmo tipo em curso |
| `J-E1706` | 409 | traço ainda não está pronto |
| `J-E1707` | 409 | operação perturbadora com saídas ativas exige `allow_live` |
| `J-E1708` | 400 | URI de descoberta ou de equipamento inválido |
| `J-E1800` | 404 | dispositivo desconhecido |
| `J-E1801` | 409 | o dispositivo não é `joker-link` |
| `J-E1802` | 503 | MCU desligado |
| `J-E1803` | 422 | artefacto incompatível com o MCU |
| `J-E1804` | 409 | OTA do MCU em curso |
| `J-E1900` | — | mensagem WebSocket mal formada ou subprotocolo errado |
| `J-E1901` | — | `op` desconhecido |
| `J-E1902` | — | tópico desconhecido ou sem papel para ele |
| `J-E1903` | — | subscrições a mais, ou orçamento de amostragem esgotado |
| `J-E1904` | — | subscrição inexistente |
| `J-E1905` | — | `min_interval` abaixo do mínimo |
| `J-E1906` | — | cliente lento: ligação fechada (4008) |
| `J-E1907` | — | sessão não retomável |
| `J-E1908` | — | batimento em falta (4002) |
| `J-E1909` | 404 | sessão desconhecida |
| `J-E1910` | 409 | estado do enrolamento não permite a operação |
| `J-E1911` | 422 | certificado não corresponde à chave ou cadeia inválida |
| `J-E1912` | 403 | enrolamento só pelo canal de arranque (o socket Unix, local ou por SSH) |
| `J-E1913` | 401 | código de enrolamento (`Joker-Enroll-Code`) errado, expirado ou já usado |
| `J-E1920` | 422 | envelope selado inválido |
| `J-E1921` | 404 | segredo não declarado |
| `J-E1930` | 409 | o sinal não é de um dispositivo `sim` |
| `J-E1940` | 409 | o agente está desligado neste nó (sem autoridade) |

Os J-codes da validação (J01xx–J07xx) aparecem dentro de `J-E1601` e, sozinhos, na conversão de valores (`J0107`, `J0108`). Os da confiança e dos segredos (`J0906`, `J0915`–`J0926`, ref/02) e o das sessões (`J0986`, ref/10) aparecem tal como estão. Não há códigos novos fora desta gama.

---

## 8. Comportamento esperado

### 8.1 Relação com o núcleo

- **A API é um serviço `svc` (documento 01, §2) e só fala com o núcleo por filas limitadas e leituras seqlock** (documento 01, §1; ADR 0013). Uma escrita manual, um método, um reset ou uma intenção aprovada entram pela inbox do grupo dono como uma proposta async; nada da API corre numa thread `rt`.
- **Nenhum caminho de escrita da API contorna o árbitro e o envelope** (invariante 3). Todos os pedidos que mudam uma saída entram no registo de decisões (invariante 8).
- As regras de arbitragem, envelope, qualidade e traço são as do documento 16. Esta referência não as repete: diz só por onde entram.

### 8.2 Ordem e consistência

- Pedidos do mesmo principal à mesma saída são serializados pela API pela ordem de chegada. Pedidos de principais diferentes à mesma saída também, e o árbitro resolve a prioridade.
- O resultado de uma escrita é o do tick em que foi aplicada. A pré-visualização é uma previsão; entre ela e o tick podem mudar entradas, níveis e leases.
- `GET /v1/values` sem `coherent` lê cada slot de forma atómica, mas não os vários slots de uma vez. Com `coherent`, as amostras vêm da mesma época seqlock (documento 02, §6.4), e as que partilham `acq_id` vêm do mesmo pedido ao driver (§6.2).
- Uma resposta traz sempre `Joker-Generation`; um cliente que junte dados de duas respostas sabe se atravessou uma troca de geração.

### 8.3 Reinícios e falhas

- O `jokerd` reinicia (OTA da runtime, watchdog): ligações e sessões perdem-se, e o `boot_id` muda. Os bilhetes de escrita perdem-se; os níveis manuais **não**: persistem a reinícios com o prazo em UTC (D4, 16 A14), e um que expirou durante o reinício não volta. Os trips, as gerações, os trabalhos, os traços e a auditoria sobrevivem.
- Um grupo encravado: as escritas ficam `queued` e respondem 202 ao fim de `wait_timeout`; o supervisor trata o grupo (documento 01, §20).
- Disco cheio: a auditoria e o registo de decisões são os últimos a perder escrita (documento 01, §20); a criação de traços é recusada (`J-E1703`).

### 8.4 Segurança

- TLS 1.3, sem 0-RTT, sem renegociação. Os certificados de cliente são verificados contra a CA de clientes do nó e contra a lista de revogação que a configuração do nó fornecer (ref/02).
- Todas as recusas (papel, origem, anti-replay, identidade) ficam na auditoria com o principal completo.
- A API não devolve credenciais, chaves privadas nem valores de segredos em nenhuma resposta, incluindo erros e logs.

---

## 9. A `jk` como cliente

### 9.1 Mapeamento de todos os comandos do documento 07

**Local** = não toca num nó. **Local + nó** = corre no portátil e usa a API quando há `--node`.

| Comando | Onde | Endpoints |
|---|---|---|
| `jk init`, `jk new`, `jk app new` | local | — |
| `jk fmt`, `jk lsp`, `jk config show`, `jk config set`, `jk ca init`, `jk ca bundle`, `jk key gen` | local | — |
| `jk check` | local; com `--node`, também no nó | `POST /v1/generations?dry_run=true` |
| `jk build [--target]`, `jk build --target mcu:<placa> [--generic \| --generation \| --lib \| --crate]` | local | — |
| `jk flash [--identity]`, `jk trace-dump --probe` | local (sonda) | — |
| `jk dev [--node] [--panel]` | local (sim) ou nó | `POST /v1/blobs/…`, `POST /v1/generations` (`base: null`), `…/activate`, `GET /v1/ws` (`signals`, `logs`), `GET /v1/sim`, `PUT`/`DELETE /v1/sim/inputs/{path}` |
| `jk test [--fuzz]` | local | — |
| `jk bench [<app>]` | nó | `POST /v1/blobs/…`, `POST /v1/bench`, `GET /v1/jobs/{id}` |
| `jk bench --target mcu:<placa>` | local (sonda ou série, ref/08 §2.12.5) | — |
| `jk diff [--against]` | local + nó | `GET /v1/generations/{gen}/manifest`, `…/lock`, `…/provenance` |
| `jk explain <chave\|código\|pipeline>` | local; `<chave>` com `--node` usa o nó | `GET /v1/generations/current/provenance?key=` |
| `jk explain <instância>` | nó | `GET /v1/instances/{i}?explain=true` |
| `jk explain <saída>` | nó | `GET /v1/outputs/{path}` (`OutputState`: `group`, `levels`, `active_level`, `envelope`, `last_outcome`) |
| `jk inspect <app.wasm\|driver.wasm\|firmware.jkfw\|model.jkm>` | local; com `--node`, a app instalada | `GET /v1/apps/{app}` (`meta`) |
| `jk lock [--update] [--check]` | local + nó | `POST /v1/lock/probe`, `GET /v1/lock/facts`, `GET /v1/lock/drift`, `GET /v1/generations/current/lock`, `GET /v1/runtime/drivers` (os drivers do binário para o lock, ref/06 §3.8) |
| `jk sim <cenário>` | local | — |
| `jk node enroll <alvo> --ca [--code]` | nó (canal de arranque: o socket Unix, local ou por SSH) | `POST /v1/node/enrollment/csr`, `PUT /v1/node/enrollment/certificate`, `GET /v1/node` |
| `jk node enroll <alvo> --deck <url> --token <t>` | nó | `POST /v1/node/enrollment/deck`, `GET /v1/jobs/{id}` |
| `jk node enroll <mcu> --link <dispositivo> [--probe]` | local (sonda, USB) + nó | `POST /v1/secrets` (a `K_link` como `secret:link.<dispositivo>`) |
| `jk node enroll mcu:<porta> --deck <url> --token <t>` | local (USB) + Deck | — (o MCU autónomo fala com o Deck pelo perfil micro, ref/10 §2.14) |
| `jk node renew` | nó | `POST /v1/node/renew` |
| `jk node trust show \| push \| set` | nó | `GET /v1/node/trust`, `PUT /v1/node/trust/bundle`, `PUT /v1/node/trust/anchors` |
| `jk secret set \| get \| list \| rm \| import`, `jk secret recipient add --user \| rm` | local | — |
| `jk secret recipient add --node` | nó | `GET /v1/node` (a cadeia do certificado com a chave KEM no SAN, verificada pela `jk` contra a CA da organização; ref/02 §3.4.2) |
| `jk secret push --node`, `jk secret status --node` | nó | `POST /v1/secrets`, `GET /v1/secrets` |
| `jk agent status \| desired \| pause \| resume \| import` | nó | `GET /v1/agent`, `GET /v1/agent/desired`, `POST /v1/agent/pause`, `…/resume`, uploads + `POST /v1/agent/import` |
| `jk session list \| accept \| close` (1.3) | nó | `GET /v1/agent/sessions`, `POST …/{s}/accept`, `DELETE …/{s}` |
| `jk doctor [--ml] [--rt] [--allow-live]` | nó | `POST /v1/doctor` (`allow_live`), `GET /v1/jobs/{id}`, `GET /v1/doctor` |
| `jk apply [--confirm] [--dry-run]` | nó | `POST /v1/blobs/missing`, uploads, `POST /v1/secrets` (as entradas de segredos que a geração refere), `POST /v1/generations[?dry_run=true]`, `POST /v1/generations/{gen}/activate` |
| `jk confirm` | nó | `POST /v1/generations/confirming/confirm` |
| `jk rollback [<geração>]` | nó | `POST /v1/generations/rollback` |
| `jk generations` | nó | `GET /v1/generations` |
| `jk runtime upgrade <versão>` | nó | uploads (`joker.runtime.v1`), `POST /v1/runtime/upgrades`, `GET /v1/jobs/{id}`, `GET /v1/runtime`, `GET /v1/info` (`boot_id`) |
| `jk runtime build` | local | — |
| `jk migrate` | local | — |
| `jk status [--instances]` | nó | `GET /v1/status[?instances=true]` |
| `jk top` | nó | `GET /v1/ws` (`status`, `metrics`, `outputs`) |
| `jk read <seletor> [--watch]` | nó | `GET /v1/values`; `--watch`: `GET /v1/ws` (`signals`) |
| `jk read <seletor> --refresh` | nó | `POST /v1/values/refresh` |
| `jk write <saída> <valor> --ttl [--level] --reason [--takeover]` | nó | `POST /v1/outputs/{path}/preview` → confirmação → `POST /v1/outputs/{path}/manual` (`If-Match`; `--takeover` é o campo `takeover`) |
| `jk release <saída> [--level]` | nó | `POST /v1/outputs/{path}/release` |
| `jk trace <sinal>` | nó | `GET /v1/signals/{path}/lineage`, `GET /v1/ws` (`signal_trace`) |
| `jk decisions […] [--report pdf\|csv]` | nó (+ local para PDF) | `GET /v1/decisions` (`text/csv` para relatórios) |
| `jk alarms [--active]` | nó | `GET /v1/alarms[?state=active]`, `GET /v1/alarm-events` |
| `jk alarm ack <id>` | nó | `POST /v1/alarms/{id}/ack` |
| `jk alarm shelve <id> <ttl> "<motivo>"` | nó | `POST /v1/alarms/{id}/shelve` |
| `jk alarm unshelve <id>` | nó | `POST /v1/alarms/{id}/unshelve` |
| `jk trips` | nó | `GET /v1/trips` |
| `jk reset <saída> --reason` | nó | `POST /v1/outputs/{path}/reset` |
| `jk intents` | nó | `GET /v1/intents` |
| `jk intent approve <id>` | nó | `GET /v1/intents/{id}?preview=true` → `POST /v1/intents/{id}/approve` |
| `jk intent reject <id>` | nó | `POST /v1/intents/{id}/reject` |
| `jk action <alvo>.<ação> [args] --key` | nó | `GET /v1/actions?target=`, `POST /v1/actions/{alvo}/{ação}`, `GET /v1/ws` (`invocations`), `POST /v1/invocations/{id}/cancel` |
| `jk call <instância>.<método> [args] --ttl --reason` | nó | `GET /v1/classes/…` (argumentos posicionais → nomes), `POST …/methods/{m}/preview` → `POST …/methods/{m}` |
| `jk app start\|stop\|restart <app>` | nó | `POST /v1/apps/{app}/start\|stop\|restart` |
| `jk device stop\|start <dispositivo> [--reason]` | nó | `POST /v1/devices/{device}/stop\|start` |
| `jk app logs <app>`, `jk logs [--app] [--follow]` | nó | `GET /v1/logs`; `--follow`: `GET /v1/ws` (`logs`) |
| `jk history <seletor> --from --to [--step]` | nó | `GET /v1/history` |
| `jk export … --parquet` | nó + local | `GET /v1/history` (`application/x-ndjson`); o Parquet é escrito pela `jk` |
| `jk record --for [--groups] --out` | nó | `POST /v1/traces` (`record`), `GET /v1/traces/{id}`, `GET /v1/traces/{id}/file` |
| `jk replay <traço> […]` | local | — (o traço vem de um dos três comandos de traço) |
| `jk trace-dump --group --last --out` | nó | `POST /v1/traces` (`ring`), `…/file` |
| `jk trace-from-history <intervalo> --app --out` | nó | `POST /v1/traces` (`history`), `…/file` |
| `jk link status` | nó | `GET /v1/uplinks` |
| `jk model build\|inspect\|migrate` | local | — |
| `jk model bench <m> --node` | nó (1.1) | `POST /v1/models/{model}/bench` (reservado) |
| `jk model lineage <m>` | local; com `--node`, 1.1 | `GET /v1/models/{model}` (reservado) |
| `jk train <job> [--now]`, `jk train status` | nó (1.1) | `POST /v1/training`, `GET /v1/training` (reservados) |
| `jk shadow start <app> <candidato>` | nó | uploads, `POST /v1/shadows` |
| `jk shadow status\|diff\|promote\|abort` | nó | `GET /v1/shadows/apps/{app}`, `…/diff`, `POST …/promote`, `POST …/abort` |
| `jk shadow … <modelo>` | nó (1.1) | `/v1/shadows/models/{model}` (reservado) |
| `jk discover <uri>` | nó | `POST /v1/discovery`, `GET /v1/jobs/{id}` |
| `jk profile import <formato> <ficheiro>` (incluindo `csv`), `jk profile check`, `jk profile diff` | local | — |
| `jk board show <placa>`, `jk board new <nome> --from <placa>`, `jk board check <ficheiro>` | local | — |
| `jk board probe --node <nó> [--name] [--out]` | nó (só leituras) | `GET /v1/node/hardware`, `GET /v1/info` (placa declarada, para comparar) |
| `jk board check <ficheiro> --node <nó>` | local + nó (só leituras) | `GET /v1/node/hardware` |
| `jk driver test`, `jk driver meta` | local | — |
| `jk profile test <perfil> --uri <uri> [--node]` | local (driver no portátil) ou nó | `POST /v1/profile-tests` (a URI da ref/03 §2.2.20 vai tal e qual em `uri`), `GET /v1/jobs/{id}` |
| `jk profile publish` | Hub | — (não é o nó) |
| `jk publish --manifest … --registry …` | local + registo OCI (o armazém do Deck) | — (não é o nó; o Deck lê do registo, documento 20 §4.4.2) |
| `jk class list [--std\|--project]`, `jk class show` | local; com `--node`, as da geração | `GET /v1/classes`, `GET /v1/classes/{pacote}/{classe}` |
| `jk class new`, `jk class fmt\|check\|diff\|doc\|gen` | local | — |
| `jk class verify <classe> --instance <i>` | local (sim) ou nó (equipamento) | `POST /v1/instances/{i}/verify` |
| `jk class publish`, `jk board publish` | Hub | — |

Referidos fora do documento 07: `jk ui` (1.3; servidor local da `jk`, cliente desta API), `jk class extract` (local, depois da v1). `jk device configure` só existe na proposta `proposals/class-idl.md` (§11).

### 9.2 Códigos de saída (documento 07, §8)

| Situação na API | Saída |
|---|---|
| 2xx; escrita com `accepted`, `clamped_*`, `suppressed`; ação `succeeded` | 0 |
| 2xx com `overridden` | 0, com aviso ("o nível X continua a mandar") |
| 400 (J-E101x, J-E103x, J-E1101, J-E1106…) | 1 |
| 422 de validação (`J-E1601`, `J0107`, `J0108`) | 2 |
| 401, 403, 409 de permissivo, ocupação ou assinatura; escrita `rejected` ou `tripped`; ação `failed` | 3 |
| sem ligação, 503, 504, 429 depois de 3 tentativas | 4 |
| `confirm` recusado porque a geração está `rolled_back`; `jk apply --confirm` que espera e vê o rollback | 5 |
| `verify` ou `profile_test` com casos falhados | 6 |
| deriva (`jk lock --check`, local, ou `GET /v1/lock/drift` com entradas) | 7 |

### 9.3 Obrigações da `jk` como cliente

- Enviar sempre `Joker-Nonce` e `Joker-Timestamp` em pedidos mutantes, mesmo pelo socket (custa nada e simplifica o código).
- Repetir um pedido mutante depois de uma falha de rede com um **nonce novo** e a **mesma** `Idempotency-Key`.
- Consultar `GET /v1/version` e `features` ao abrir a sessão e recusar com mensagem clara uma opção que o nó não suporta, antes de receber um `J-E1011`.
- Mostrar sempre o `code` dos problemas, para o `jk explain` funcionar (ref/00, §8).

---

## 10. Exemplos com `curl`

Os exemplos usam o socket Unix. Do portátil, o socket chega-se por SSH, e aí o anti-replay é opcional (§2.5):

```sh
ssh -N -L /tmp/porto.sock:/run/joker/api.sock joker-admin@revpi-porto.local &
J='curl -sS --unix-socket /tmp/porto.sock'
H='-H Content-Type:application/json'
```

Em TCP com mTLS, cada pedido mutante leva os dois cabeçalhos:

```sh
T='curl -sS --cert ops.crt --key ops.key --cacert org-ca.crt'
$T -X POST https://revpi-porto.local:7443/v1/alarms/grid_overfreq/ack \
   -H "Joker-Nonce: $(ulid)" -H "Joker-Timestamp: $(date -u +%FT%TZ)" \
   -H Content-Type:application/json -d '{"reason":"visto"}'
```

(`ulid`: qualquer gerador de ULID.)

### 10.1 Do portátil ao campo (documento 07, §9)

```sh
# jk new, jk dev, jk test: locais.

# jk node enroll revpi-porto.local --ca ~/org-ca
$J -X POST http://joker/v1/node/enrollment/csr $H -d '{"node":"revpi-porto"}' | jq -r .csr_pem > porto.csr
#   (a jk assina porto.csr com a CA local e obtém porto.crt)
$J -X PUT http://joker/v1/node/enrollment/certificate $H \
   -d "$(jq -n --rawfile c porto.crt --rawfile ca clients-ca.crt '{chain_pem:$c, client_ca_pem:$ca}')"

# jk doctor --rt
$J -X POST http://joker/v1/doctor $H -d '{"checks":["base","rt"],"rt_duration":"60s"}'
#   → 202, Location: /v1/jobs/01J8Z4…
$J http://joker/v1/jobs/01J8Z4Q0M6K2V7X9B1C3D5F7GH | jq '.state, .result.summary'

# jk discover sunspec://192.168.10.30
$J -X POST http://joker/v1/discovery $H -d '{"uri":"sunspec://192.168.10.30"}'

# jk lock --node: o manifesto ainda não está no nó
$J -X POST http://joker/v1/lock/probe $H \
   -d "$(jq -n --rawfile m /tmp/efetivo.toml '{manifest:{content:$m}}')"
#   → trabalho lock_probe; result.lock_toml é o excerto [discovered.*]/[fallback.*]

# jk bench: carregar a app e medir
D=sha256:$(sha256sum target/peak_shaver.wasm | cut -d' ' -f1)
$J -X POST http://joker/v1/blobs/missing $H -d "{\"digests\":[\"$D\"]}"
$J -X POST "http://joker/v1/blobs/uploads?digest=$D&kind=joker.app.v1%2Bwasm" \
   -H Content-Type:application/octet-stream --data-binary @target/peak_shaver.wasm
$J -X POST http://joker/v1/bench $H -d "$(jq -n --rawfile m /tmp/efetivo.toml --arg d "$D" \
   '{manifest:{content:$m}, apps:[{digest:$d, kind:"joker.app.v1+wasm", name:"apps.peak_shaver"}]}')"

# jk check: local (e, se se quiser, no nó)
$J -X POST 'http://joker/v1/generations?dry_run=true' $H -d @proposal.json | jq .plan

# jk apply --confirm 10m
BASE=$($J -D - -o /dev/null http://joker/v1/generations/current | awk -F'"' '/^etag/I{print $2}')
$J -X POST http://joker/v1/generations $H -d @proposal.json      # base = $BASE; → 201, gen 18
$J -X POST http://joker/v1/generations/18/activate $H -H "If-Match: \"$BASE\"" \
   -d '{"confirm_window":"10m","reason":"primeiro deploy"}' | jq '.state, .confirm_deadline'

# jk status
$J http://joker/v1/status | jq '.generation, .devices, .alarms.active'

# jk confirm
$J -X POST http://joker/v1/generations/confirming/confirm $H -d '{}'
```

`proposal.json`:

```json
{
  "manifest": { "digest": "sha256:1f0c…" },
  "lock": { "digest": "sha256:77ab…" },
  "provenance": { "digest": "sha256:c3d2…" },
  "artifacts": [
    { "digest": "sha256:9f2c…", "kind": "joker.app.v1+wasm", "name": "apps.peak_shaver" },
    { "digest": "sha256:0b1e…", "kind": "joker.profile.v1+toml", "name": "profiles/janitza/umg604@2.profile.toml" },
    { "digest": "sha256:5a6b…", "kind": "joker.key.v1+pem", "name": "keys/site-ops.pub" }
  ],
  "config": { "digest": "sha256:d41f…" },
  "base": "sha256:8e7d…",
  "message": "site-porto: primeiro deploy",
  "source": { "repo": "git@git.example.com:sites/porto.git", "commit": "4c1d2e9", "dirty": false }
}
```

### 10.2 Um incidente

```sh
# jk decisions --output pcs.p_set --since "2026-09-24 03:00" --to "03:30"
$J 'http://joker/v1/decisions?output=pcs.p_set&from=2026-09-24T03:00:00Z&to=2026-09-24T03:30:00Z&order=asc' \
   -H Accept:text/csv > decisoes.csv

# só as escritas que leram meter.p_total
$J 'http://joker/v1/decisions?output=pcs.p_set&cause=meter.p_total&since=1h' | jq '.items[] | {ts, outcome, applied}'

# jk trace-from-history "03:00..03:30" --app peak_shaver --out incidente.jktrace
$J -X POST http://joker/v1/traces $H \
   -d '{"source":"history","from":"2026-09-24T03:00:00Z","to":"2026-09-24T03:30:00Z","app":"peak_shaver"}'
#   → 201, id 01J8Z5…, approximate: true, warnings
$J -o incidente.jktrace http://joker/v1/traces/01J8Z5B2N4P6R8T0V2X4Z6A8CD/file

# ou, exato, do anel contínuo do grupo
$J -X POST http://joker/v1/traces $H -d '{"source":"ring","groups":["fast"],"from":"2026-09-24T03:00:00Z","to":"2026-09-24T03:30:00Z"}'

# jk replay incidente.jktrace --diff --app peak_shaver@1.4.3: local
```

### 10.3 Operação do dia a dia

```sh
# jk read 'pcs.**'
$J 'http://joker/v1/values?sel=pcs.**' | jq '.items[] | [.path, .value, .unit, .quality, .age]'

# jk read 'pcs.**' --watch (SSE, sem WebSocket)
$J -N 'http://joker/v1/stream?topic=signals&sel=pcs.**&min_interval=500ms'

# jk write pcs.p_set "50 kW" --ttl 30m --reason "ensaio" : pré-visualização, depois escrita
$J -X POST http://joker/v1/outputs/pcs.p_set/preview $H \
   -d '{"value":"50 kW","ttl":"30m","reason":"ensaio"}' | jq '.prediction, .conflict'
$J -X POST http://joker/v1/outputs/pcs.p_set/manual $H -H 'If-Match: "sha256:8e7d…"' \
   -H "Idempotency-Key: $(uuidgen)" -d '{"value":"50 kW","ttl":"30m","reason":"ensaio"}'
#   → {"id":"01J8…","state":"committed","level":"manual:4","outcome":{"outcome":"clamped_rate","applied":12.5},…}

# jk release pcs.p_set
$J -X POST http://joker/v1/outputs/pcs.p_set/release $H -d '{"reason":"fim do ensaio"}'

# jk alarm shelve grid_overfreq 2h "manutenção"
$J -X POST http://joker/v1/alarms/grid_overfreq/shelve $H -d '{"ttl":"2h","reason":"manutenção"}'

# jk reset pcs.p_set --reason "falha BMS resolvida"
$J -X POST http://joker/v1/outputs/pcs.p_set/reset $H -d '{"reason":"falha BMS resolvida"}' | jq .result

# jk action pcs.reset_fault --key k1
$J -X POST http://joker/v1/actions/pcs/reset_fault $H -H 'Idempotency-Key: k1' -d '{"args":{}}'

# jk call p1.run_at 60% --ttl 10m --reason "arranque manual"
$J -X POST http://joker/v1/instances/p1/methods/run_at $H \
   -d '{"args":{"speed":"60 %"},"ttl":"10m","reason":"arranque manual"}'

# jk intent approve 01J8…
$J -X POST http://joker/v1/intents/01J8Z6C3P5R7T9V1X3Z5B7D9FG/approve $H -d '{"reason":"ok, piso 3"}'
```

### 10.4 Sombra de uma app (o fluxo "um modelo novo" do §9, na versão 1.0)

O fluxo do documento 07 usa um modelo, que é da 1.1. Com uma app, na 1.0:

```sh
# jk shadow start peak_shaver peak-shaver-1.5.0.wasm
$J -X POST http://joker/v1/shadows $H -H 'If-Match: "sha256:8e7d…"' -d '{
  "target": "app:peak_shaver",
  "candidate": { "digest": "sha256:aa01…", "kind": "joker.app.v1+wasm", "name": "apps.peak_shaver" },
  "until": { "duration": "48h", "max_divergence": "5 kW p95" },
  "reason": "versão 1.5.0 em sombra"
}'

# jk shadow status peak_shaver (48 h depois)
$J http://joker/v1/shadows/apps/peak_shaver | jq '.state, .divergence, .criteria'

# jk shadow promote peak_shaver
$J -X POST http://joker/v1/shadows/apps/peak_shaver/promote $H -H 'If-Match: "sha256:31c9…"' \
   -d '{"reason":"divergência p95 de 1,2 kW"}'
```

---

## 11. Estresse

**S1 — Um painel subscreve `**` num nó com um grupo de 1 ms e 5 000 sinais, por Wi-Fi.** O amostrador lê os slots sem espera a cada 100 ms e envia só os `seq` novos. O Wi-Fi não aguenta: a fila da ligação enche, o amostrador deixa de gerar mensagens enquanto ela não esvazia, e a taxa efetiva cai para o que a rede aguenta. Ao fim de 30 s de fila cheia, 4008. O núcleo não vê nada disto. **Achado:** 1 000 clientes a `**` a 10 Hz eram 50 M leituras/s na `bg`, mais do que o CPU. **Correção incorporada:** orçamento de amostragem do nó (1 M/s, `J-E1903`) e o limite de 10 000 sinais por subscrição.

**S2 — A rede cai a meio de um `jk write`.** A `jk` não sabe se a escrita chegou. Repete com um nonce novo e a mesma `Idempotency-Key`: a API devolve o bilhete guardado (`Joker-Idempotent-Replay: true`), e o registo de decisões tem uma entrada, não duas. Sem a chave, a repetição só renovava o lease (o mesmo valor, o mesmo nível): inofensivo, mas com duas decisões. **Aguenta.**

**S3 — Dois operadores escrevem na mesma saída ao mesmo tempo, os dois em `manual:4`.** O primeiro ganha; o segundo recebe `J-E1105` com o nome, o prazo e o motivo do primeiro. **Achado:** o documento 16, A5, proíbe empates num nível, mas só como erro de validação; um nível manual é ocupado em runtime por pessoas. **Correção incorporada:** ocupação por principal, `takeover` explícito, e as duas decisões no registo.

**S4 — A geração muda entre a pré-visualização e a confirmação.** A pessoa viu `range = ±125 kW`; entretanto o Deck aplicou `±100 kW`. A `jk` manda `If-Match` com o hash que viu: 412 `J-E1026`, e a `jk` mostra a pré-visualização nova. **Aguenta.**

**S5 — Esquecer o TTL, ou pôr 30 dias.** `J-E1101` com a ajuda "escritas manuais levam sempre `--ttl` (A10); o máximo desta saída é 8h". `ttl: "30d"` é `J-E1102`, com o `max_manual`. A API não arredonda um TTL para baixo: um operador que pediu 30 dias tem de saber que não os teve. **Aguenta.**

**S6 — `jk apply` e o Deck ao mesmo tempo.** A `jk` prepara a geração 18 sobre a 17; o agente recebe um estado desejado e começa a trocar para outra. O trinco de transição dá `J-E1608` à segunda, e o `If-Match` da `jk` falharia de qualquer modo. **Aguenta.**

**S7 — A geração nova muda o ponto de escuta TCP e corta a própria API.** O plano marca `api` em `confirm_reasons`; em `prod`, `activate` sem janela é `J-E1610`. Com `--confirm 10m`, a `jk confirm` já não chega ao nó, a janela expira e a geração anterior volta sozinha. A `jk` que ficou à espera vê `rolled_back` e sai com 5. **Achado:** o ADR 0015 fala de rede, segurança e uplinks, e não da própria API. **Correção incorporada:** `api` é uma razão de confirmação à parte.

**S8 — Um pedido capturado é reenviado por um relé comprometido.** Em TCP, o nonce repetido dá `J-E1022`; um pedido novo com um `Joker-Timestamp` velho dá `J-E1021`. **Achado:** com o relógio `unsynced` (arranque sem RTC e sem NTP), a janela não se pode verificar, e recusar tudo tirava ao operador o controlo exatamente quando mais precisa. **Correção incorporada:** só o nonce, memória alargada a 10 min, `Joker-Clock: unsynced` e a auditoria marcada. **Achado 2:** uma memória de nonces cheia que esquecesse os mais antigos abria uma janela de replay. **Correção:** recusar (`J-E1029`), nunca esquecer.

**S9 — Um navegador com o certificado de cliente instalado visita uma página maliciosa.** A página tenta um `POST` para `https://node.example.com:7443/v1/outputs/…/manual`. O navegador junta o certificado; a API recusa pelo `Origin` (`J-E1004`) e pelo tipo de conteúdo não JSON de um formulário (`J-E1008`), e sem CORS a página nem lê a resposta. **Aguenta**, e é a razão para não haver CORS na 1.0.

**S10 — Versões misturadas.** Uma `jk` 1.3 envia `{"at": …}` (escrita agendada, 1.1) a um nó 1.0: `J-E1017`, e não uma escrita imediata em silêncio. Uma `jk` 1.0 contra um nó 1.3 recebe campos novos e ignora-os. **Achado:** a regra da ref/00 ("os leitores toleram o desconhecido"), aplicada aos pedidos, faria um nó velho ignorar uma opção de segurança nova. **Correção incorporada:** o servidor é estrito com os pedidos (`J-E1011`), os clientes toleram as respostas.

**S11 — Uma consulta de histórico a 5 000 sinais, 7 dias em cru.** Resolve para 5 000 sinais (dentro do limite), mas são milhões de pontos: `J-E1038` com a sugestão de `step` ou de NDJSON paginado. Em NDJSON, a resposta sai em fluxo, página a página, com a leitura do historian em `bg`. **Aguenta.**

**S12 — Erros comuns de unidades e tipos.** `jk write pcs.p_set 50` escreve 50 **kW** (a unidade do sinal); `"50 W"` é convertido para 0,05 kW; `"50 Hz"` é `J0107`; `"50 var"` é `J0108`; `true` num `f64` e `"1"` num `bool` são `J-E1036`; um `u64` acima de 2⁵³ vai como string. A pré-visualização mostra sempre o valor já convertido, na unidade da saída, antes de a pessoa confirmar. **Aguenta.**

**S13 — Retoma depois de um corte curto numa subscrição de alarmes.** Um corte de 20 s: o `resume` reenvia as transições da réplica. Um corte de 5 min: `resume_unavailable`, um `resync` com a tabela atual, e as transições do intervalo vêm de `/v1/alarm-events`. Nenhuma transição se perde sem aviso. **Aguenta.**

---

## 12. Decisões e alternativas rejeitadas

| Decisão | Porquê | Rejeitado |
|---|---|---|
| Sem escritas por WebSocket | anti-replay, idempotência, auditoria e erros num só sítio | escritas por WS: dois caminhos para a mesma semântica |
| SSE como espelho de uma subscrição | `curl` e scripts sem biblioteca WS; o motor é o mesmo | SSE com vários tópicos por ligação (é o WebSocket outra vez) |
| Tópicos de estado amostrados, eventos enfileirados | um cliente lento recebe menos, nunca trava nada, e os eventos não se perdem em silêncio | uma fila por cliente para tudo (a memória cresce com a lentidão) |
| O resultado do envelope é 200 | o pedido foi bem feito; a decisão é dado, não erro | 409 em `rejected` (mistura erros de uso com decisões do núcleo) |
| Nível manual ocupado por principal, com `takeover` | um operador não apaga o override de outro sem saber | "o último ganha" (um operador apaga, sem saber, o override de outro) |
| `manual:4` por omissão | é o que a trilha mostra (parte 2) e deixa `manual:3` para escaladas | `manual:5` |
| Sombra como gerações derivadas; rollback como **reativação** da geração guardada (D9) | um só mecanismo de configuração, com histórico de ativações e confirmação; o mesmo hash e o mesmo número em todo o lado | sombra como estado de runtime fora das gerações; rollback como geração nova (dois números para o mesmo conteúdo) |
| Upload em blocos à OCI, armazém por conteúdo | retomável, deduplicado, familiar | multipart com tudo num pedido (rebenta os 16 MiB) |
| O nó não gera Parquet, Arrow nem PDF | binário e CPU do nó; a `jk` e as bibliotecas cliente convertem o NDJSON (ref/05 §23) | exportação no nó; Arrow em `queryHistory` (pedido da ref/05) |
| Pré-visualização como operação própria (`previewManualWrite`, `previewMethodCall`) | a pré-visualização é uma leitura (`viewer`), sem anti-replay nem auditoria de escrita | `dry_run` no corpo da escrita (pedido da ref/05): a mesma operação com dois papéis |
| Sem tokens na 1.0; identidade pelo transporte | menos uma credencial a guardar e a revogar | JWT para o Studio (entra pelo túnel na 1.3) |
| CORS desligado e `Origin` recusado | um navegador com certificado é um vetor real | lista de origens na 1.0 |
| Pedidos estritos, respostas tolerantes | uma opção nova nunca é ignorada em silêncio | tolerância nos dois sentidos |
| `viewer` por omissão para quem chega | o socket e a CA já são a decisão de confiança | "sem regra, sem acesso" (obriga a listar todos os leitores) |
| Intenções pendentes expiram no TTL delas | quem propõe diz quanto tempo a proposta vale | prazo fixo de aprovação |
| `refresh` exige `operator` | gera tráfego no barramento | `viewer` |
| Descoberta, `bench`, `doctor` só `admin` | tocam na rede de campo ou no núcleo `rt` | `operator` |

---

## 13. Marcações de versão

| Versão | O quê |
|---|---|
| **1.0** | tudo o que está nos §2–§11, exceto o que segue |
| 1.0, vazio até à 1.1 | `/v1/intents` (os escritores só-proposta, M18, chegam na 1.1) |
| 1.0, `skip` até à 1.3 | `doctor` com `network` |
| **1.1** (Preparado para ML) | `/v1/models/**`, `/v1/training`, `/v1/shadows/models/**`, `/v1/signals/{path}/blocks`, `/v1/streams`; tópicos `frames` e `blocks`; `shape` `block` e `stream`; `kind` `joker.model.v1+jkm`; `ManualWriteRequest.at` (M6); `Metrics.ml`; `Decision.model_outputs` (M22); `PlanChange.kind = model`. Tudo já está na representação e responde `J-E1017` na 1.0 |
| **1.3** | transporte `tunnel` e sessões remotas do Deck (`/v1/agent/sessions`); lista de origens (CORS) para o Studio; tokens de sessão, se forem precisos; `doctor` `network` (modo gateway) |
| 2.0 | caminhos qualificados por nó (`@<nó>.…`, documento 21, §8): prefixo `/v2` |
