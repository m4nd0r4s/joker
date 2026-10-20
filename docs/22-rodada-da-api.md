# 22 — Rodada da API: as interfaces fechadas

Esta rodada pegou em **todas as interfaces** do Joker e escreveu-as como referência exaustiva, para que a implementação trabalhe só sobre especificações. O resultado está em `docs/ref/` (onze documentos) e nos ficheiros máquina-legíveis em `wit/` e `spec/`. Este documento conta o que se decidiu, onde as referências se contradiziam e como se resolveu, e o que se atacou de ponta a ponta.

---

## 1. O que existe agora

| Referência | Superfície | Máquina-legível |
|---|---|---|
| `ref/00-convencoes.md` | representação comum: versões, nomes, tempo, valores, códigos numéricos, identificadores, codificações, erros, segurança, limites | — |
| `ref/01-manifesto.md` | `joker.toml` e overlays: todas as secções e chaves, gramática dos valores, manifesto efetivo | (esquema JSON gerado dos tipos, 16 §10) |
| `ref/02-lock-no-e-cliente.md` | `joker.lock`, `/etc/joker/node.toml`, `/var/lib/joker/`, cliente `jk`, segredos, confiança | `spec/cddl/secrets.cddl` |
| `ref/03-perfis-placas-sim.md` | perfis, placas, simulação, cenários, conformidade (núcleo e classes), pacotes e Hub | `wit/joker-sim.wit` |
| `ref/04-wit-e-sdk-rust.md` | mundo `joker:runtime`, ABI de ciclo, `joker:meta`, SDK Rust, `joker-test`, erros `J-E0xxx` | `wit/joker-runtime.wit`, `spec/cddl/joker-meta.cddl` |
| `ref/05-sdk-python-typescript.md` | SDKs de tier 2 (Python, TypeScript) e bibliotecas cliente (Rust, Python, TypeScript) | — |
| `ref/06-drivers.md` | tier N (traits), tier W (`joker:driver`), tier X (JDP), suite de conformidade de drivers | `wit/joker-driver.wit`, `spec/cddl/jdp.cddl`, `spec/c/jdp.h` |
| `ref/07-joker-link.md` | Joker Link ao nível do fio: enquadramento, segurança, sessão, as mensagens (lista na ref/07 §6.1), OTA, orçamentos | `spec/cddl/joker-link.cddl` |
| `ref/08-joker-micro-abi.md` | ABI C, crate `joker-micro`, modo firmware, blob `.jkgen`, descritor `.jkfw`, alvos | `spec/c/joker_node.h`, `spec/c/example/`, `spec/cddl/joker-micro.cddl` |
| `ref/09-api-http.md` | API HTTP local, WebSocket e SSE (as operações são as do OpenAPI) | `spec/openapi/joker-api.yaml` |
| `ref/10-protocolo-do-agente.md` | nó ↔ Deck: enrolamento, estado desejado, relatórios, eventos, sessões, HTTPS e MQTT, perfil micro | `spec/cddl/agent.cddl` |

**Validação feita:** todos os blocos TOML, JSON e YAML passam num parser; o OpenAPI passa no `openapi-spec-validator`; os três pacotes WIT (`joker:runtime`, `joker:driver`, `joker:sim`) passam no parser oficial (via `jco`), e o `joker:driver` resolve contra o `joker:runtime`; os headers C compilam com `-std=c11 -Wall -Wextra -pedantic` e em C++17; o CDDL do `joker:meta` foi validado com `zcbor` contra instâncias, e os restantes passam num parser de CDDL; os vetores de teste do Joker Link (CRC, COBS, ChaCha20-Poly1305, COSE) foram gerados e verificados por código.

**O que ainda não foi compilado:** as assinaturas Rust e os blocos TypeScript foram revistos à mão, e os CDDL, exceto o do `joker:meta`, só passaram num parser de sintaxe. Isto passa a ser a tarefa 0 da Fase 0 (§8).

---

## 2. O mapa das interfaces

```
            pessoas                                          máquinas
 ┌──────────────────────────┐                   ┌────────────────────────────────┐
 │ joker.toml, overlays      │ ref/01            │ Joker Deck                     │
 │ perfis, placas, sim,      │ ref/03            │   ▲ agent/1 (JSON/HTTPS,       │
 │ cenários, conformidade    │                   │   │  CBOR/MQTT) ref/10         │
 │ classes .jkc              │ 17                └───┼────────────────────────────┘
 └────────────┬─────────────┘                        │
              │ jk check / build / lock (ref/02)     │
              ▼                                      │
 ┌──────────────────────────────────────────────────┴───────────────────────────┐
 │ jokerd                                                                        │
 │  API HTTP/WS ref/09 ◀── jk, Studio, clientes Rust/Python/TS (ref/05)          │
 │  apps Wasm ── joker:runtime (ref/04; SDKs Rust ref/04, Python/TS ref/05)      │
 │  drivers ── tier N traits · tier W joker:driver · tier X JDP (ref/06)         │
 │  dispositivo joker-link ──────────────┐                                       │
 └───────────────────────────────────────┼───────────────────────────────────────┘
                                         │ Joker Link ref/07 (COBS+CRC-32C, CBOR)
                                         ▼
                        ┌───────────────────────────────────────┐
                        │ Joker Micro: firmware · lib C · crate │ ref/08
                        │   blob .jkgen, joker_node.h            │
                        └───────────────────────────────────────┘
            tudo com a mesma representação: ref/00 · a mesma semântica: 16
```

---

## 3. Um valor de ponta a ponta

A potência de um inversor atrás de um RP2350, lida pelo MCU às 10:00:00,1238 UTC com a qualidade `Uncertain(Stale)`. O mesmo valor em cada interface:

| Onde | Forma | Referência |
|---|---|---|
| registo Modbus no MCU | `u16` cru `0x101D`, escala 0,1 | ref/03 §2.2, ref/06 §2.7 |
| slot no MCU | `value` = bits de `412.5_f64`; `quality` = `0x0000_0301` (código 1, motivo 3) | 01 §3.2, ref/00 §4.3 |
| Joker Link, `SAMPLES` | `[40, 5120, 412.5, 1800, 769]`: `sid`, `seq` de 16 bits, valor, `dt` em µs, qualidade | ref/07 §6.9 |
| slot no `jokerd` | `ts_acq` = instante monotónico (ns) da receção no nó Linux; `ts_hw` = `t0 + dt` do MCU, convertido para o monotónico do nó pelo desvio e pela taxa que o `TIME` estima, com a flag de domínio "link"; `seq` do slot do nó (a de 16 bits do MCU só serve para descartar amostras velhas) | ref/07 §8.3, §8.4, 01 §3.2 |
| app Rust, ciclo | `inv.p.get()` → `Err(QualityError { port, quality: Quality::uncertain(Stale) })`; `get_uncertain()` → `Ok(412.5)` (na unidade da porta, kW) | ref/04 §4.6, §4.7.1, 16 §3.1 |
| app TypeScript | `inv.p.get()` lança `QualityError`; `inv.p.tryGet()` → `{ ok: false, … }` | ref/05 §6.1 |
| traço | 16 B: `seq` u32, qualidade u32, valor u64 | 16 §7 |
| API HTTP | `{"path":"inv.p","value":412.5,"unit":"kW","quality":"uncertain","reason":"stale","ts_hw":"2026-09-24T10:00:00.123800000Z","ts_acq":"2026-09-24T10:00:00.126100000Z","seq":88123}` (`ts_hw` é o instante do MCU, `ts_acq` o da receção; `seq` é a do slot do nó) | ref/00 §4.2, ref/09 §3.3 |
| Deck | não recebe a amostra: recebe a telemetria (Sparkplug B) e, no relatório, a saúde do dispositivo | ref/10 §2.10, 20 §3.3 |

Em nenhum ponto a qualidade se perde, a unidade muda sem conversão explícita, ou um instante monotónico sai do nó sem o par UTC.

## 4. Uma escrita de ponta a ponta

Uma operadora força a bomba de um MCU a parar, a partir do Studio no portátil, por TCP com mTLS:

1. `POST /v1/outputs/pump.run_cmd/manual` com `value: false`, `ttl: "2h"`, `reason`, `level` por omissão `manual:4`, e os cabeçalhos `Joker-Nonce` e `Joker-Timestamp` (ref/09 §5.6, §2.5).
2. O `jokerd` verifica o papel `operator`, o anti-replay e a ocupação do nível (16 A13). Como a saída é de um MCU, o nó Linux **não** a arbitra nem lhe aplica envelope (`J0234`, ref/07 §8.1): guarda o TTL e o nível e trata a escrita como um escritor remoto `user:<principal>` (16 A10, A12).
3. O árbitro do **MCU** é o que decide: o `jokerd` envia `PROPOSE` no nível delegado ao link, com `lease = min(TTL restante, max_lease)` (omissão 5 s; ref/07 §6.12, §8.1, 16 A12). O MCU aplica o envelope dele e devolve `OUTCOMES` com `accepted` (valor 1, ref/00 §4.4).
4. A resposta HTTP é `200` com o `WriteTicket` e o resultado, que se resolve no primeiro `OUTCOMES` que cobre o `PROPOSE` (ref/07 §8.2). Um `clamped_rate` ou `rejected(interlock)` também é `200`, porque é um resultado do envelope e não um erro do pedido (ref/09 §3.4).
5. O `jokerd` renova o lease no MCU, em pedaços de `max_lease`, enquanto o TTL durar. Se o cabo cair, o lease expira **no MCU** em ≤ `max_lease`, e o MCU passa ao nível ativo seguinte ou ao `safe` (ref/07 §7.4).
6. Se o `jokerd` reiniciar, o nível manual é reposto a partir do disco com o prazo em UTC (16 A14), e volta a ser enviado ao MCU: a paragem para manutenção não desaparece com um reinício. Sem relógio `synced` no arranque, repõe-se o TTL restante e levanta-se o alarme `joker.output.<s>.manual_restored`.
7. O Deck vê tudo isto só no relatório (`overrides`, os níveis manuais ativos, ref/10 §2.10.1) e nos eventos `decision` das escritas manuais (ref/10 §2.11); a auditoria da API fica no nó. **O protocolo do agente não tem nenhuma mensagem de escrita em sinais** (ref/10 D9).

---

## 5. Decisões transversais

Onde as referências, escritas em paralelo, divergiam ou deixavam uma escolha, decidiu-se assim. As referências e os documentos existentes foram alinhados com estas decisões (§7).

| # | Decisão | Onde se aplica |
|---|---|---|
| **D1** | **Precedência:** `ref/00` tem o nível do 16; `ref/01`–`ref/10` ficam **acima** dos documentos por área (01–05, 14, 19, 20), que passam a ser a explicação e remetem para a referência no detalhe. ADR 0023. | docs/README, AGENTS |
| **D2** | **Registos em CBOR** (eventos, argumentos e resultados de ações e métodos, `Internal<T>`): mapa com **chaves inteiras = posição do campo** no esquema (`joker:meta` ou contrato `.jkc`). Os campos só se acrescentam no fim dentro de uma major. O JSON usa os nomes. | ref/00 §4.1, ref/04 §4.10, ref/07 §6.2, 17 §9 |
| **D3** | **Expressões:** o curto-circuito com qualidade é **simétrico** (`false && Bad` e `Bad && false` dão ambos `false (Good)`), porque os operandos não têm efeitos. `any`/`all` generalizam o curto-circuito; `count`/`sum`/`maxs`/`mins` dão a pior qualidade. Aritmética de inteiros em `f64`, comparações entre inteiros exatas. `own(x)` sem proposta anterior devolve o valor aplicado. | 02 §4, 16 §3.2, §4, 17 §4.1 |
| **D4** | **Regras novas do núcleo:** A12 (escritores remotos: posse por nível delegado, lease contado no consumidor), A13 (dois principais no mesmo nível manual: o segundo é recusado sem `takeover`), A14 (os níveis manuais persistem a reinícios, com o prazo em UTC), E10 (proposta NaN, infinita ou com enum inexistente é `Rejected(WrongType)`). | 16 §1, §2 |
| **D5** | **Escritas agendadas (M6) e intenções (M18) são 1.1** em todas as superfícies: reservadas no manifesto (`J0290`), `@unstable` no WIT, endpoints que respondem vazio ou `J-E1017`, reservadas no MCU (`J0783`). | 01, 02, 03, 04, 07, ref/* |
| **D6** | **Segredos cifrados por destinatário (HPKE)**, com assinatura por entrada: o `secrets.toml.enc` **pode** ir para o Git, e é assim que o Deck os distribui sem os conseguir ler. Nas apps: `[apps.<app>.secrets] <nome_local> = "secret:<nome>"`. ADR 0024. | 01 §17, 03 §1, 15 §2.4, ref/01, ref/02, trilha |
| **D7** | **Identidade e assinaturas:** a identidade do nó é **P-256** (TPM e elementos seguros suportam-na); as chaves da organização que assinam artefactos e documentos são **Ed25519**; COSE_Sign1 aceita `EdDSA` e `ES256`. Documentos assinados vão sempre em CBOR/COSE, também sobre HTTPS. A confiança vem de um bundle assinado por uma raiz fora de linha; `[security.trust]` no manifesto só restringe. ADR 0025. | ref/00 §7, 01 §17, 03 §13, 20 |
| **D8** | **Um lock por manifesto de entrada:** `joker.rpi.toml` dá `joker.rpi.lock`. | 15, trilha antiga 05 |
| **D9** | **Rollback reativa a geração anterior** com o mesmo hash e o mesmo número; o histórico é de ativações, cada uma com `origin`. `generation_state` ganha `discarded`. | 01 §15, ref/09, ref/10 |
| **D10** | **Um só esquema de conformidade** para o núcleo e para as classes (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), com os literais canónicos em minúsculas (`clamped_rate`, `bad(comm_failure)`) e as formas da CLI como sinónimos exatos. | 16 §8, 14 §7.3, ref/03 §2.6 |
| **D11** | **`schema = 1`** é o primeiro par chave/valor dos ficheiros de catálogo e de teste (comentários antes são permitidos). | ref/00 §1, exemplos, trilha |
| **D12** | **CRC-32C** (Castagnoli) em todo o lado: Joker Link, blob de geração, registos de persistência no MCU. | 19 §6, ref/07, ref/08 |
| **D13** | **Tipos de media numa só tabela** (ref/00 §7.1), com a forma completa `application/vnd.joker.<tipo>.v1+<sufixo>`, incluindo `runtime`, `config`, `driver` (wasm e tar), `secrets`, `manifest`, `lock`, `provenance`, `cwasm`, `conformance`. | 20 §3.2 |
| **D14** | **SDK Rust:** `Many<C, MIN, MAX>` (a forma `1..=8` não é Rust válido); `.mins()` (evita o `Ord::min`); alocar no ciclo é trap `J-E0903` (o alocador bump deixava ponteiros pendentes); sai o `#[joker::shared]`; `by_name()`; `clamped(clamp)` e `record error`. | 04, 14, trilha 06–08 |
| **D15** | **Tier 2 também no ciclo**, com período mínimo de 100 ms em TypeScript e 500 ms em Python, e admissão pelo **máximo** medido (as pausas de GC tornam o p99,9 enganador). Plano B se o async do WASI 0.3 não estiver pronto nos toolchains: variante bloqueante numa thread `app`. | 04 §4, ref/02, ref/05 |
| **D16** | **Anti-replay** (nonce e `ts`) na API em TCP, no túnel e no agente. As pontes autenticam-se pelo protocolo delas, com nível e lease. A janela de ±30 s só vale com o relógio `synced`; sem ele, valem o nonce e a ligação ao alvo, e isso fica na auditoria. | ref/00 §9, 01 §17 |
| **D17** | **Identificadores estáveis entre gerações:** o *path-hash* (primeiros 8 B do SHA-256 do caminho canónico) liga a interface externa de um MCU a cada geração. O `SignalId` continua denso e válido só numa geração. | ref/00 §5, ref/08 |
| **D18** | **Nomes de pacotes:** Python `joker_sdk`, `joker_client`, `joker_testing`, `joker_classes` (`import joker` levaria a instalar o pacote errado do PyPI); npm `@joker/*` (**confirmar a organização**); crates novos `joker-client`, `joker-agent`, `joker-agent-proto` (`no_std`), `joker-hal`, `joker-driver-sdk`, `joker-jdp`. | 13 |
| **D19** | **Escrita manual:** nível por omissão `manual:4`; `--reason` obrigatório; `--to` (e não `--until`) nos intervalos. | 07 |
| **D20** | **Drivers:** o núcleo executa os passos dos perfis (`connect`, `relinquish`, fallback M1) sobre duas primitivas do driver (`read-addr`, `write-addr`); a conversão cru ↔ engenharia é toda do núcleo; os tier X arrancam como unidades systemd transitórias com `DynamicUser`. | 05, 01 §17 |
| **D21** | **Joker Link:** a segurança configura-se nos dois lados, sem negociação (não há *downgrade*); três classes de entrega (efémera, estado, fiável); modos de sessão `full`, `observe`, `maintenance`, `refused`; `SAMPLES` retransmite o valor atual, nunca o antigo. | 19 §6 |
| **D22** | **ABI C:** dois headers (`joker_node.h` fixo, `joker_project.h` gerado); o código C escreve como uma app `kind = "extern"`; um *panic* de app num MCU é fatal (`jk_fatal`), porque não há sandbox; o trip é gravado em flash até `service_deadline` depois de acontecer. | 19 §4 |
| **D23** | **Agente:** o estado desejado é completo e idempotente, com `seq`; sem fila de ordens; fases por ordem fixa (runtime → MCU → geração); uma geração só se confirma sozinha depois de um `report-ack` do Deck (uma geração que cortou a rede não se confirma a si própria); proteções locais contra um Deck comprometido (janela de manutenção, `max_applies_per_day`, `local_changes = "hold"`). | 20 |

---

## 6. Estresse transversal

Os casos de cada superfície estão na sua referência (8 a 12 por documento). Aqui ficam os que atravessam várias.

**X1 — Versões desalinhadas em todo o lado.** Deck 1.5, nó 1.3, MCU com firmware 1.0, app compilada para `joker:runtime@1.1`, `jk` 1.4 no portátil. O agente negoceia `agent/1` e o Deck limita-se ao que um 1.3 entende (ref/10 §3.14); a app 1.1 corre num 1.3 (o mundo só cresce); o MCU anuncia `link_version` 1.0 no `HELLO` e o `jokerd` fala 1.0 com ele (ref/07); a `jk` 1.4 lê `GET /v1/version` e as `features` do `GET /v1/info` e recusa, com uma mensagem clara, as opções que o nó não tem (ref/09 §9.3, ref/05 §21). **Aguenta**, porque cada superfície tem a sua versão e a regra "leitores toleram o desconhecido, pedidos são estritos" (ref/00 §1).

**X2 — Um sinal muda de nome entre gerações.** `pump.speed` passa a `pump.speed_ref`. O `SignalId` muda (é denso por geração); as apps recebem handles novos no `init` da geração; a interface externa de uma biblioteca C num MCU é ligada por nome (com o *path-hash* da secção `PATHS`) a cada geração, e uma geração a que falte uma entrada obrigatória é recusada pelo `jk build --generation` (`J0790`) e, se chegar ao MCU, pelo `jk_init` (`JK_E_GEN_FIRMWARE`, `J-E2516`), em vez de ligar a variável errada (ref/08 §2.8.2). **Aguenta.**

**X3 — Relógio `unsynced` do princípio ao fim.** Um nó arranca sem RTC nem rede. O controlo corre em monotónico e não muda (01 §18); os instantes UTC saem marcados com o relógio `unsynced`, e as apps com `requires_clock = "synced"` ficam com as saídas bloqueadas (`Rejected(ClockUnsynced)`); a API aceita escritas por TCP só com nonce e regista-o na auditoria (D16); o `TIME` leva ao MCU a qualidade `unsynced`, e o MCU fica `unsynced` (ref/07 §8.4); o agente não verifica `issued` nem `expires`, aceita o estado desejado pelo `seq`, e as fases com janelas esperam (`wait = clock`; ref/10 §3.12). **Aguenta**, com a regra "sem relógio, nada que dependa dele decide em silêncio".

**X4 — Um segredo tenta sair por uma porta lateral.** Pelo manifesto da geração (`GET /v1/generations/{gen}/manifest`, que só tem referências `secret:<nome>`), pelo traço (os segredos nunca são sinais), pelo `joker:meta` (só nomes), pelo relatório ao Deck (nomes e `rev`, nunca valores), por um log de app (a interface `secrets` só existe em async, e o SDK marca os valores como não imprimíveis). **Aguenta**; o risco que fica é a própria app enviar o segredo por rede, que só pode para os destinos de `net.allow` (ref/01).

**X5 — Um cliente WebSocket lento, um Deck lento e um MCU lento ao mesmo tempo.** Três filas independentes, nenhuma no caminho de controlo: o WebSocket amostra os slots (um cliente lento só recebe menos vezes) e fecha com 4008 ao fim de 30 s de fila cheia; o agente tem store-and-forward com perdas marcadas (`gap`); o Joker Link coalesce amostras (`coalescidas` no `SAMPLES`). **Aguenta**; o que se perde é sempre contado.

**X6 — A mesma app em quatro linguagens.** O peak shaver em Rust, TypeScript e Python (ref/05 §20 mostra-o lado a lado) e, num MCU, a mesma lógica em C como app `extern` (o padrão de `spec/c/example/`, ref/08 §2.17). A rodada encontrou dois bugs no exemplo original do documento 04 (limite de 0 kW com o plano ainda por calcular; porta `grid` contra `grid_power` no `bind`), que foram corrigidos. **Quebrou, corrigido.**

**X7 — Um erro de escrita num ficheiro.** `max_leas = "2s"`. É `J0140` com sugestão (`max_lease`), porque os ficheiros escritos por pessoas são estritos; num JSON da API, um campo desconhecido num pedido é `J-E1011`; num CBOR recebido de um MCU mais novo, é ignorado. **Aguenta**: estrito onde erra uma pessoa, tolerante onde evolui uma máquina.

**X8 — Um Deck comprometido.** Consegue enviar um estado desejado assinado pela chave `deck`, mas os artefactos precisam de assinaturas do pipeline, que não vivem no Deck; não consegue escrever em sinais (não há mensagem para isso); não consegue alargar envelopes nem mudar a configuração (uma geração nova tem de vir assinada pela chave `config`, que não vive no Deck, e passa pelo `jk check` no nó); não consegue ler segredos (HPKE para os nós). Consegue, no máximo, voltar a gerações e runtimes antigas já assinadas, acima dos pisos, limitado por `max_applies_per_day` e pela janela de manutenção local. **Aguenta**, com o risco residual declarado na ref/10 §4 (S1).

**X9 — Um integrador escreve um driver e uma classe e publica no Hub.** Driver tier W validado pela suite de conformidade de drivers (ref/06 §6), classe validada pela suite de conformidade de classes (ref/03 §2.6), relatórios assinados como referrers OCI, perfil com relatório de hardware real (ref/03 §2.8). O Hub recusa publicações sem relatório (`J0849`, `J0880`). **Aguenta.**

**X10 — Replay de um incidente com um MCU pelo meio.** O traço do MCU chega por `TRACE` (em ns, sem saídas de ML), os escritores remotos aparecem a partir de `0x8000_0000` (16 §7), e o replay no portátil reproduz os resultados do MCU byte a byte, porque o núcleo é o mesmo (ADR 0020). **Aguenta.**

---

## 7. Achados nos documentos existentes

As onze referências registaram mais de cem achados (as listas que ficaram depois da integração estão na ref/01, anexo C, na ref/03, na ref/04 §9 e na ref/05 §33): contradições entre documentos, exemplos que não compilariam, chaves usadas sem estar definidas, erros de aritmética e dois bugs de lógica. Cada referência tem a sua lista, com a mudança exata. **Foram todos aceites e aplicados**, com as resoluções da §5 onde havia conflito. Os mais importantes:

| Documento | O que mudou |
|---|---|
| 16 | A12–A14, E10; curto-circuito simétrico; agregações; inteiros; `own()`; traço com escritores remotos e MCU em ns; §8 passa a remeter para o esquema único de conformidade |
| 02 | curto-circuito simétrico; `Clamped` com os dois valores; M6 marcado 1.1 |
| 03 | `secrets` como tabela; `trust` só restringe; `mtls` sai (TCP é sempre mTLS); chaves 1.1 marcadas; `"**alarm.**"` → `"alarm.**"`; grupo `default` alinhado com A11 |
| 04 | WIT substituído por remissão para `wit/`; propostas de 14 B; sem alocação no ciclo; `Many<C, MIN, MAX>`; `.mins()`; peak shaver corrigido; tier 2 com mínimos |
| 05 | trait novo, `joker:driver` e JDP por remissão para a ref/06 |
| 07 | comandos novos das referências; `--to`; `manual:4`; `--reason`; intenções marcadas 1.1 |
| 14, 17 | `Many`; nomes reservados de membros; métodos contra membros; idempotência só sem `own()`; bug da rotação de bombas no §9.1 |
| 19 | CRC-32C, TCP, `TIME` bidirecional, exemplo C com `joker_project.h`, *panic* fatal, orçamento do U1 |
| 20 | tipos de media; princípio 2 reescrito; rollback por `reject` |
| 01, 13, 15 | segredos, identidade P-256, drivers X por systemd, crates novos, layout de `/var/lib/joker` |
| exemplos, trilha (antiga) | `schema = 1`; `joker.rpi.lock`; `.gitignore` sem o `secrets.toml.enc`; chaves 1.1 marcadas; sintaxe do SDK atualizada |

---

## 8. O que isto pede à Fase 0

A rodada fecha as specs **no papel**. A Fase 0 fecha-as **no código**, antes de qualquer funcionalidade:

1. **Tarefa 0a — compilar as referências.** Um crate `joker-spec-tests` que (1) compila os blocos Rust das referências como doctests, (2) corre `tsc` nos blocos TypeScript, (3) valida os CDDL com um validador completo contra os exemplos, (4) gera o esquema JSON do manifesto a partir dos tipos e valida contra ele todos os blocos TOML dos documentos, (5) valida o OpenAPI, (6) verifica as contagens que a prosa ainda repete (códigos, operações, mensagens) contra as fontes (`spec/codes.toml`, OpenAPI, CDDL; documento 13, §5.1). Falha a CI se uma referência e o código divergirem.
2. **Tarefa 0b — o registo de códigos.** A semente já existe: `spec/codes.toml`, com as entradas extraídas das tabelas (ativos e retirados; o ficheiro é a fonte das contagens), uma gravidade por código e sem colisões. A tarefa passa-a para `crates/joker-manifest/codes.toml`, escreve as mensagens do produto e liga o teste de CI (16 §9). A rodada criou a maior parte dos códigos (os que estão nas tabelas das referências e não estavam nas dos documentos por área; as contagens são as do `spec/codes.toml`, verificadas pelo `joker-spec-tests`), em gamas separadas por referência (ref/00 §8), sem colisões: os dois códigos que aparecem em duas referências (`J0109`, `J0720`) têm o mesmo significado nas duas.
3. **Tarefa 0c — os vetores de teste.** Os do Joker Link (ref/07 §4.8), os da ABI C (ref/08) e os casos de conformidade do núcleo passam a ficheiros em `conformance/`, corridos em Linux e num MCU.

Continuam a ser decididas na Fase 0 as questões do documento 16, §10 (admissão RT exata, `dt()` após `skip`, transições a partir de `Unknown`, orçamento de barramentos além do RTU). A base de `extends` já não está em aberto: a ref/01 fecha-a (os caminhos de `extends` são relativos ao ficheiro que os declara, `J0160`), e o esquema JSON do manifesto é gerado a partir dos tipos.

**Continua em aberto, e não é técnico:** a licença, o nome e a marca, a organização `@joker` no npm, a equipa por frente, o hardware de CI e o modelo de negócio (documento 18, §6).
