# ref/00 — Convenções transversais das interfaces

Este documento fixa as regras que **todas** as interfaces do Joker partilham: ficheiros TOML, `.jkc`, WIT, SDKs (Rust, Python, TypeScript), drivers (tier N, W, X), Joker Link, ABI C, API HTTP e protocolo do agente. Cada referência em `docs/ref/` aplica estas regras e não as repete. Uma referência que as contradiga está errada.

É **normativo**, com a mesma precedência do documento 16 (ADR 0023). Tudo o que é semântica (árbitro, envelope, qualidade, expressões) continua no 16. Aqui fica só a **representação**.

---

## 1. As superfícies e as suas versões

| Superfície | Identificador de versão | Onde está a referência |
|---|---|---|
| runtime (`jokerd`, `jk`) | SemVer do produto (`1.4.2`) | — |
| manifesto, overlays | `[project] joker = "1.4"` (versão mínima da runtime) | ref/01 |
| lock, `node.toml`, configuração do cliente, `secrets.toml.enc` | `version = 1` | ref/02 |
| perfis, placas, simulação, cenários, conformidade | `schema = 1` como **primeiro par chave/valor** (só comentários e linhas vazias antes) | ref/03 |
| classes `.jkc` | versão da classe (`pump@1`) + `jkc = 1` implícito | 17 |
| mundo das apps | pacote WIT `joker:runtime@0.x.y` até à 1.0; **`joker:runtime@1.0.0` na 1.0** | ref/04 |
| drivers tier W | pacote WIT `joker:driver@…`, com a **mesma versão** do `joker:runtime` que importa; `@unstable` na 1.0 e na 1.1 (só drivers da árvore), estável na 1.2 | ref/06 §4 |
| drivers tier X (JDP) | `jdp` inteiro no `hello` e no `welcome` (1 na v1); o `jokerd` suporta o corrente e o anterior | ref/06 |
| SDKs Rust, Python, TypeScript | SemVer do pacote; o SDK declara o mundo WIT que gera | ref/04, ref/05 |
| Joker Link | `link_version` (u8 major, u8 minor) no `HELLO` | ref/07 |
| ABI C do Joker Micro | `JK_ABI_VERSION_MAJOR` / `JK_ABI_VERSION_MINOR` | ref/08 |
| blob de geração (`.jkgen`) e descritor de firmware (`.jkfw`) | `format` e `expr_isa` no cabeçalho | ref/08 |
| API HTTP local | prefixo `/v1`; `info.version` no OpenAPI | ref/09 |
| protocolo do agente (nó ↔ Deck) | `agent/1` | ref/10 |
| artefactos OCI | tipo de media com `.v1` (tabela do §7.1) | §7.1 |

**Regra de compatibilidade (ADR 0022) aplicada à representação:**

1. Numa minor só se **acrescenta**: campos opcionais, mensagens novas, variantes novas no fim de um enum, endpoints novos.
2. **Leitores toleram o desconhecido** nas interfaces máquina-a-máquina (JSON, CBOR, WIT via adaptador): campos desconhecidos ignorados; variante de enum desconhecida mapeada para a variante `unknown` ou para a categoria (uma razão de qualidade desconhecida conta pela sua `quality-code`). Isto vale para quem **lê respostas e mensagens**. Um servidor é estrito com os **pedidos** que recebe: um campo desconhecido num pedido HTTP é `J-E1011`. Cada caso novo de um enum WIT declara a sua projeção para os mundos anteriores (ref/04 §2.8).
3. **Os ficheiros escritos por pessoas são estritos:** uma chave desconhecida num TOML é um erro (`J0140`), porque quase sempre é um erro de escrita. Chaves de extensão começam por `x_` e são ignoradas.
4. Um número de campo, uma chave CBOR inteira ou um código **nunca se reutiliza** depois de removido.

---

## 2. Nomes

| Onde | Estilo | Exemplo |
|---|---|---|
| chaves TOML, chaves JSON, valores de enum em JSON | `snake_case` | `max_lease`, `"comm_failure"` |
| caminhos de sinais, nomes de dispositivos, apps, instâncias, grupos | `snake_case`, gramática do documento 02, §9 | `pump1.run_cmd` |
| WIT | `kebab-case` (regra do WIT) | `quality-reason`, `write-at` |
| Rust | convenções Rust (`snake_case`, `CamelCase`) | `last_outcome()`, `WriteOutcome` |
| Python | PEP 8 | `last_outcome()`, `WriteOutcome` |
| TypeScript | `camelCase` para a API do SDK, `PascalCase` para tipos | `lastOutcome()`, `WriteOutcome` |
| C | prefixo `jk_` para funções e tipos, `JK_` para macros e constantes | `jk_tick`, `JK_GOOD` |
| erros e avisos | códigos do documento 16, §9 | `J0107`, `J-E0901` |

**Nomes vindos do utilizador não se traduzem.** Portas, parâmetros, membros de classes, sinais e variantes de enums declarados pelo utilizador mantêm-se `snake_case` em todas as linguagens, incluindo TypeScript (`pump.run_cmd`, `inputs.grid_p`). Só a API do SDK segue o estilo da linguagem. Assim um nome encontra-se com um `grep` em todos os ficheiros de um projeto.

**Reservados:**

- caminhos: o primeiro segmento `joker` (a runtime) e `board` (IO fixo das placas, `board.<nome>`); o primeiro segmento `@` (caminhos qualificados por nó na v2, documento 21, §8);
- nomes de apps `joker*`; o nome de nó `local`; chaves TOML começadas por `_`;
- nomes de portas de apps, em todas as linguagens: `now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta`;
- nomes de membros de classes: `name`, `index`, `health`, `read`, `raw`, `has` (documento 17, §3.2).

**Valores de enum em TOML** escrevem-se em `snake_case` (`"clamped_rate"`, `"bad(comm_failure)"`). As formas de apresentação da CLI (`Clamped(Rate)`, `Bad(CommFailure)`) são aceites como sinónimos exatos nos cenários e nos casos de conformidade, e o `jk fmt` normaliza-as.

**Nomes de ficheiros** (ADR 0026). O tipo de cada ficheiro está no nome, na forma `<nome>.<tipo>.toml`; o conteúdo tem de bater com o sufixo (`J0803`, ref/03 §2.1.6):

| Tipo | Nome | Exemplo | Referência |
|---|---|---|---|
| manifesto | `joker.toml` | `joker.toml`, `micro/esp/joker.toml` | ref/01 |
| overlay | `joker.<variante>.toml`, em qualquer pasta | `joker.lab.toml`, `templates/joker.bess-v2.toml` | ref/01 §7.7 |
| lock | `joker.lock`; `joker.<variante>.lock` para cada overlay de entrada | `joker.lab.lock` | ref/02 §2.1.1 |
| perfil de equipamento | `<modelo>@<major>.profile.toml` | `profiles/local/bench-esp32@1.profile.toml` | ref/03 §2.2 |
| perfil de placa | `<placa>.board.toml` | `boards/rpi-zero-w.board.toml` | ref/03 §2.3 |
| modelo de simulação | `<nome>.sim.toml` | `sim/esp.sim.toml`, `classes/std/valve.sim.toml` | ref/03 §2.4 |
| cenário | `<nome>.scenario.toml` | `sim/scenarios/falha_esp.scenario.toml` | ref/03 §2.5 |
| suite de conformidade | `<nome>.conformance.toml` | `conformance/envelope/rate.conformance.toml` | ref/03 §2.6 |
| fixture de drivers | `<driver>.fixture.toml` | `drivers/modbus/conformance/modbus-tcp.fixture.toml` | ref/06 §6.2 |
| descritor de driver (fonte TOML) | `<driver>.driver.toml` | `joker-drv-acme.driver.toml` | ref/06 §5.8 |
| pacote de classes | `<pacote>.package.toml` | `classes/acme/water/water.package.toml` | ref/03 §2.7 |
| classe | `<nome>.jkc` (`<nome>@<major>.jkc` com duas majors) | `classes/std/pump.jkc` | 17 |
| nó, cliente, Deck, frota, segredos, registo de códigos | nomes fixos | `node.toml`, `config.toml`, `deck.toml`, `fleet.toml`, `secrets.toml.enc`, `codes.toml` | ref/02, ref/10 |

`<variante>` segue a gramática dos nomes de placa (`[a-z0-9]+ ("-" [a-z0-9]+)*`, ≤ 32 caracteres). As referências escrevem-se sem sufixo quando são nomes (`profile = "local/bench-esp32@1"`, `board = "rpi-zero-w"`: a resolução acrescenta-o) e com ele quando são caminhos (`model = "sim/esp.sim.toml"`).

---

## 3. Tempo

| Grandeza | Binário (WIT, traço, JDP) | Microcontrolador (ABI C, Joker Link) | Texto (TOML, JSON, CLI) |
|---|---|---|---|
| instante monotónico | `u64` ns | `u64` µs, época do arranque do MCU | string decimal (`"mono_ns": "81234567890"`) |
| instante UTC | `s64` ns desde 1970-01-01T00:00:00Z | `s64` µs (só depois de `TIME`) | RFC 3339, `Z`, até 9 casas (`"2026-09-24T10:00:00.123456789Z"`) |
| duração | `u64` ns | `u32` µs (até ~71 min) ou `u32` ms (campos `_ms`) | literal de duração (§3.1) |
| qualidade do relógio | enum `clock-quality` | idem, numérico (§6) | `"synced"`, `"holdover"`, `"unsynced"` |

- Os segundos intercalares não existem: UTC é contado como o POSIX conta.
- Um instante monotónico **só tem sentido no nó que o produziu** e na geração do relógio (documento 01, §18). Nunca sai do nó sem o par UTC ao lado.
- O `jokerd` converte µs ↔ ns na fronteira do Joker Link.
- **Exceção do Joker Link:** as mensagens de um MCU levam só o monotónico em µs. O par UTC é estabelecido pelo `TIME` e guardado no nó Linux, que faz a tradução (ref/07 §6.23).
- Em TypeScript, `i64`, `u64` e instantes em ns são sempre `bigint` (o monotónico em ns passa 2⁵³ ao fim de ~104 dias).

### 3.1 Literais de duração

```
duração := número espaço? unidade
unidade := "ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"
```

- `m` e `min` são minutos **só onde se espera uma duração** (chaves de duração, argumentos de janela, `--ttl`). Numa quantidade física, `m` é o metro. Nunca há ambiguidade, porque o tipo da posição é conhecido.
- Não há compostos (`1h30m`): escreve-se `"90m"`.
- Decimais permitidos (`"1.5s"`); o valor é arredondado a ns.
- Nos **instantes** de cenários e de casos de conformidade (`at`), e só aí, aceita-se uma soma `duração + duração` (`"2m+1.2s"`); nas suites de drivers, também o termo `timeout` (`"timeout+10ms"`, ref/03 §5).
- **Forma canónica de saída** (ficheiros gerados, respostas da API, `jk fmt`): inteiro na maior unidade exata, com `m` para minutos (`"90m"`, `"1500ms"`, `"2h"`).

### 3.2 Literais de quantidade

`"125 kW"`, `"-20 kW/s"`, `"49.8 Hz"`, `"5 %"`. O espaço entre número e unidade é opcional. **No manifesto e nos ficheiros de catálogo**, uma grandeza com unidade exige sempre o literal com unidade (`J0151`); só as interfaces máquina (JSON da API) aceitam um número simples, interpretado na unidade do sinal. As unidades são as do catálogo do documento 16, §5. Tamanhos de memória e de disco usam prefixos IEC: `"512 MiB"`, `"1 GiB"`.

---

## 4. Valores, amostras e qualidade

### 4.1 Valores

| Tipo do sinal | Binário (`u64` do slot, do traço e do Link) | JSON |
|---|---|---|
| `bool` | 0 ou 1 | `true` / `false` |
| `i64` | complemento para dois | número se \|v\| < 2⁵³, senão string decimal |
| `u64` | tal qual | número se v < 2⁵³, senão string decimal |
| `f64`, `f32` | bits IEEE 754 de `f64` (`f32` promovido) | número; `NaN` e infinitos não existem em JSON e só aparecem com qualidade `bad(nan)` e `"value": null` |
| `enum<N>` | valor numérico da variante | nome da variante (`"run"`) |
| `bits<N>` | a palavra | número, e `"bits": ["alarm_overtemp", …]` com os bits ativos nomeados |
| `string`, `bytes` | arena à parte (não cabe em `u64`) | string UTF-8; `bytes` em base64 |
| registo (eventos, argumentos e resultados de ações e métodos, `Internal<T>`, mensagens de caixas de correio) | **CBOR determinístico** (RFC 8949, §4.2.1), mapa com **chaves inteiras = posição do campo no esquema** (`joker:meta` ou contrato `.jkc`); dentro de uma major, os campos só se acrescentam no fim | objeto JSON com os **nomes** dos campos |

**Unidade:** um valor numérico é sempre expresso na **unidade declarada do sinal**. As interfaces de texto mostram a unidade ao lado (`"unit": "kW"`) e aceitam, na entrada, um número (na unidade do sinal) ou um literal de quantidade (`"50 kW"`, convertido; dimensão errada é `J0107`).

### 4.2 Amostra em JSON

```json
{
  "path": "meter.p_total",
  "value": 412.5,
  "unit": "kW",
  "quality": "good",
  "reason": "none",
  "ts_src": "2026-09-24T10:00:00.120000000Z",
  "ts_acq": "2026-09-24T10:00:00.123456789Z",
  "seq": 88123
}
```

`ts_src` e `ts_hw` só aparecem quando existem. O `ts_acq` em JSON é já traduzido para UTC; o monotónico está disponível pedindo `?mono=true` (ref/09). Campos opcionais, pedidos por parâmetro: `age`, `acq_id`, `bits`, `ts_acq_mono_ns`, `ts_hw_mono_ns` (ref/09 §3.3).

### 4.3 Códigos numéricos de qualidade

Usados no slot (documento 01, §3.2: código nos bits 0–7, motivo nos bits 8–15, flags nos bits 16–31), no traço, no Joker Link e na ABI C.

| Código | Valor | | Motivo | Valor | | Motivo | Valor |
|---|---|---|---|---|---|---|---|
| `good` | 0 | | `none` | 0 | | `comm_failure` | 12 |
| `uncertain` | 1 | | `overridden` | 1 | | `device_fault` | 13 |
| `bad` | 2 | | `local_override` | 2 | | `out_of_range` | 14 |
| | | | `stale` | 3 | | `config_error` | 15 |
| | | | `not_confirmed` | 4 | | `not_connected` | 16 |
| | | | `substituted` | 5 | | `nan` | 17 |
| | | | `out_of_distribution` | 6 | | `tripped` | 18 |
| | | | `sensor_cal` | 7 | | `deadline` | 19 |
| | | | `clock_unsynced` | 8 | | | |
| | | | `initial` | 9 | | | |
| | | | `frozen` | 10 | | | |
| | | | `partial_write` | 11 | | | |

A ordem é a do enum WIT `quality-reason`. Valores de 20 a 239 são reservados para motivos futuros; de 240 a 255, para uso privado de drivers (aparecem como o motivo 0 fora do nó).

### 4.4 Resultados de escrita

| Resultado | Valor | Motivo (para `rejected`) | Valor |
|---|---|---|---|
| `none` (sem proposta neste tick) | 0 | `wrong_type` | 1 |
| `accepted` | 1 | `interlock` | 2 |
| `clamped_range` | 2 | `quality` | 3 |
| `clamped_rate` | 3 | `range` | 4 |
| `suppressed` | 4 | `atomic_group` | 5 |
| `overridden` | 5 | `not_owner` | 6 |
| `deferred` | 6 | `clock_unsynced` | 7 |
| `shadowed` | 7 | `budget` | 8 |
| `rejected` | 8 | | |
| `tripped` | 9 | | |

O documento 02, §7.1, escreve `Clamped(Range|Rate)`: são os valores 2 e 3. O WIT distingue-os com `clamped(clamp)` (ref/04). Em JSON: `{"outcome": "clamped_rate", "applied": 1.0}`, `{"outcome": "rejected", "reason": "interlock"}`, `{"outcome": "overridden", "level": "manual:3"}`.

---

## 5. Identificadores

| Coisa | Forma em texto | Forma em binário | Regra |
|---|---|---|---|
| sinal | caminho (`pump1.run_cmd`) | `SignalId` `u32` | o `SignalId` é **denso e válido só numa geração**; nunca sai do nó sem o hash do registo ao lado |
| sinal, estável entre gerações | — | *path-hash*: primeiros 8 B do SHA-256 do caminho canónico | liga a interface externa de um MCU a cada geração (ref/08) |
| chave pública | chave: `x25519:<base64>`, `ed25519:<base64>`; id de chave: `ed25519:<32 hex>` (os primeiros 16 B do SHA-256 da chave pública) | 32 B; id 16 B | ref/02 §2.5 |
| nível do árbitro | `"<faixa>:<nível>"` (`"optimizer:13"`); o nível é absoluto (1–16) e tem de estar dentro da faixa | `u8` 1–16 | faixas do documento 01, §6 |
| escritor | `app:<nome>`, `user:<principal>`, `bridge:<nome>`, `impl:<instância>`, `intent:<id>`, `link:<dispositivo>` (escritor remoto num MCU, 16 A12) | `u32` por geração; no traço, os escritores remotos de um MCU são ≥ `0x8000_0000` | A1 do documento 16 |
| principal | `unix:<utilizador>`, `cert:<CN>`, `deck:<deck_id>/user:<id>`, `deck:<deck_id>/group:<g>` | — | ref/09 §2.2, ref/10 |
| geração | `gen` inteiro atribuído na criação, e `sha256:<hex>` do conteúdo | 32 B | o hash identifica, o inteiro ordena a criação; um rollback **reativa** a geração anterior (mesmo hash, mesmo `gen`) e o histórico regista a ativação com `origin` |
| digest de artefacto | `sha256:<hex>` (OCI) | 32 B | |
| nó | `[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?` (rótulo DNS) | string | único numa organização |
| site | mesma gramática | string | |
| classe | `<pacote>/<nome>@<major>` (`std/pump@1`; `std/` pode omitir-se) | hash do contrato, 32 B | documento 17, §9 |
| instância, membro | caminho (`pump1`, `pump1.start`) | `u32` instância, `u16` membro | |
| chave de idempotência | string ASCII imprimível, 1–64 B | idem | janela `dedup_window` (E9) |
| intenção, pedido, sessão | ULID (`01J8…`, 26 caracteres) | 16 B | ordenáveis por tempo |

---

## 6. Enumerações numéricas partilhadas

| Enum | Valores |
|---|---|
| `clock_quality` | `synced` 0, `holdover` 1, `unsynced` 2 |
| `alarm_state` | `normal` 0, `active_unacked` 1, `active_acked` 2, `rtn_unacked` 3, `unknown` 4 |
| `severity` | `low` 0, `medium` 1, `high` 2, `critical` 3 |
| `role` | `viewer` 0, `operator` 1, `admin` 2 |
| `writer_kind` | `cycle` 0, `async` 1, `manual` 2, `bridge` 3, `impl` 4, `proposal` 5 |
| `app_state` | `starting` 0 (a carregar, a compilar ou no `init`), `running` 1, `crashloop` 2, `stopped` 3 (parada por uma pessoa ou pela geração), `failed` 4 (o `init` falhou ou a app não carrega), `shadow` 5 (modo sombra, M16: propostas gravadas, não aplicadas), `updating` 6 (troca a quente em curso, 01 §9); é o único enum dos estados de uma app, na API (ref/09 §5.14), no OpenAPI (`AppState`) e no agente (`app-state`, ref/10 §2.2) |
| `generation_state` | `pending` 0, `confirming` 1, `confirmed` 2, `rolled_back` 3, `failed` 4, `discarded` 5 |
| `mode` | `dev` 0, `prod` 1 |
| `link_state` (dispositivos) | `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5 |
| `session_mode` (Joker Link) | `full` 0, `observe` 1, `maintenance` 2, `refused` 3 |
| `ota_kind` | `generation` 1, `firmware` 2, `model` 3 (1.1 no Linux; OTA de modelos para MCUs na 1.4), `app` 4 (1.4); 0 nunca é usado |
| tipo de sinal | `bool` 0, `i64` 1, `u64` 2, `f64` 3, `f32` 4, `enum` 5, `bits` 6, `string` 7, `bytes` 8, `record` 9 (`Internal<T>`) |

---

## 7. Codificações

| Onde | Codificação | Porquê |
|---|---|---|
| ficheiros escritos por pessoas | TOML 1.0, UTF-8 (ADR 0018) | legível, revisível em PR |
| API HTTP, WebSocket, protocolo do agente sobre HTTPS | JSON (RFC 8259), UTF-8; o agente pode negociar CBOR | depurável, ferramentas universais |
| Joker Link, protocolo do agente sobre MQTT/CoAP, JDP (controlo), `joker:meta`, argumentos e eventos no WIT | CBOR (RFC 8949) com **chaves inteiras** nas mensagens do protocolo | compacto, `no_std`, com norma |
| tudo o que é assinado ou tem hash | CBOR **determinístico** (RFC 8949, §4.2.1) | o mesmo conteúdo dá sempre os mesmos bytes |
| documentos assinados (estado desejado, bundle de confiança, blob de geração, manifesto de OTA, relatórios assinados) | COSE_Sign1 (RFC 9052), **sempre em CBOR**, também quando viajam sobre HTTPS; algoritmos `EdDSA` (Ed25519, chaves da organização) e `ES256` (P-256, chaves de identidade dos nós em TPM ou elemento seguro) | uma só família de assinaturas, também em MCUs (ADR 0025) |
| identidade TLS dos nós | certificados X.509 **P-256** emitidos pela CA da organização | suportada por TPM 2.0 e elementos seguros |
| artefactos OCI | assinaturas compatíveis com o cosign/notation, ao lado do artefacto | ferramentas de registo existentes |
| traço | binário do documento 16, §7 | |

Os esquemas das mensagens CBOR escrevem-se em **CDDL** (RFC 8610), em `spec/cddl/`. Os esquemas JSON da API HTTP estão no OpenAPI 3.1 (`spec/openapi/joker-api.yaml`). O WIT está em `wit/`. **CRC:** onde há CRC (Joker Link, blob de geração, registos de persistência no MCU), é sempre o **CRC-32C** (Castagnoli), em little-endian (ref/07 §3.2).

### 7.1 Tipos de media

A forma curta (`joker.app.v1+wasm`) é a usada nos documentos e na API; no registo OCI, o `mediaType`/`artifactType` completo é `application/vnd.joker.<forma curta sem "joker.">` (`application/vnd.joker.app.v1+wasm`).

| Forma curta | Conteúdo | Papel que assina | Referência |
|---|---|---|---|
| `joker.runtime.v1` | binário do `jokerd` (índice multi-plataforma) | `runtime` | ref/09, ref/10 |
| `joker.config.v1+cbor` | descritor de uma geração de um nó Linux (manifesto efetivo, lock, contratos, artefactos por digest) | `config` | ref/10 §2.6 |
| `joker.manifest.v1+toml`, `joker.lock.v1+toml`, `joker.provenance.v1+json` | as partes de uma geração enviadas à API | (dentro da geração) | ref/09 §5.13 |
| `joker.app.v1+wasm`, `joker.app.v1+cwasm` | componente de uma app; AOT para uma arquitetura | `apps` | ref/04 |
| `joker.driver.v1+wasm`, `joker.driver.v1+tar` | driver tier W; driver tier X (índice multi-plataforma) | `drivers` | ref/06 §4.7, §5.9 |
| `joker.profile.v1+toml`, `joker.board.v1+toml`, `joker.sim.v1+toml` | perfil, placa, modelo de simulação | `profiles`, `boards` | ref/03 §2.8 |
| `joker.class.v1+jkc`, `joker.class.contract.v1+json`, `joker.class.conformance.v1+toml` | classe e as suas camadas | `classes` | ref/03 §2.8 |
| `joker.class.package.v1+toml` | ficheiro de pacote de classes | `classes` | ref/03 §2.7 |
| `joker.conformance-report.v1+json` | relatório de conformidade (*referrer*) | quem o produziu | ref/03, ref/06 §6.5 |
| `joker.micro-firmware.v1`, `joker.generation.v1` | firmware do Joker Micro; blob de geração `.jkgen` | `firmware`; `config` | ref/08 |
| `joker.secrets.v1` | conjunto de segredos selado para um nó | `secrets` | ref/02 §2.4 |
| `joker.key.v1+pem` | chave pública | — | ref/09 |
| `joker.trace.v1` | traço `.jktrace` | — | 16 §7 |
| `joker.model.v1+jkm` | modelo de ML | `models` | **reservado 1.1** |

---

## 8. Erros

Há **um só vocabulário** de erros: os códigos do documento 16, §9, no registo `codes.toml`. Cada superfície embrulha-o à sua maneira:

| Superfície | Forma |
|---|---|
| `jk` | estilo rustc (código, localização, ajuda) e códigos de saída do documento 07, §8 |
| API HTTP | `application/problem+json` (RFC 9457) com `code` (`"J0213"`), `title`, `detail`, `status` e, quando há ficheiro, `spans` |
| WIT | `result<_, error>` com `record error { code: string, message: string }` onde houver erros que a app deva distinguir; `string` só onde a app não decide nada com ele |
| Joker Link, JDP, protocolo do agente | um `status` numérico pequeno da própria mensagem, mais `code` (string do registo) opcional |
| ABI C | `jk_status_t` (`JK_OK` = 0, erros negativos) |
| SDKs | tipos de erro da linguagem, que levam o código (`err.code == "J-E0901"`) |

Um erro visto por uma pessoa traz **sempre** o código, para que `jk explain <código>` funcione.

**Gamas novas** (acrescentam-se à tabela do documento 16, §9):

| Gama | Área |
|---|---|
| `J014x`–`J019x`, `J023x`–`J029x` | manifesto e overlays (ref/01) |
| `J041x`–`J049x` | lock (ref/02) |
| `J073x`–`J076x` | Joker Link, validação (ref/07) |
| `J077x`–`J079x` | Joker Micro, validação (ref/08) |
| `J0800`–`J0849` | perfis, placas, simulação, cenários, conformidade, pacotes (ref/03) |
| `J0850`–`J0899` | drivers tier N, W, X (ref/06) |
| `J0900`–`J0929` | nó, cliente, confiança, segredos (ref/02) |
| `J0930`–`J0999` | enrolamento, protocolo do agente, sessões (ref/10) |
| `J-E0001`–`J-E0799`, `J-E0900`–`J-E0999` | erros de runtime vistos pelas apps e do SDK Rust (ref/04) |
| `J-E0800`–`J-E0899` | SDKs de tier 2 e bibliotecas cliente (ref/05) |
| `J-E1000`–`J-E1999` | API HTTP, WebSocket, SSE e sessões (ref/09) |
| `J-E2000`–`J-E2499` | Joker Link em runtime (ref/07) |
| `J-E2500`–`J-E2999` | ABI C e crate do Joker Micro (ref/08) |

Na ABI C, `jk_status_t` é 0 em sucesso, negativo em erro e positivo em resultados informativos (`JK_EMPTY`, `JK_PENDING`, `JK_RELINQUISHED`).

---

## 9. Segurança comum

- **Papéis:** `viewer`, `operator`, `admin` (documento 01, §14). Cada endpoint, comando e mensagem declara o papel mínimo.
- **Escritas vindas de fora do nó** pela API em TCP, pelo túnel de uma sessão remota e pelo protocolo do agente levam `nonce` (ULID) e `ts` (UTC), e são rejeitadas com nonce repetido ou, com o relógio `synced`, fora de ±30 s (documento 01, §17). Com o relógio `holdover` ou `unsynced`, verifica-se só o nonce e a ligação ao alvo, e isso fica marcado na auditoria. A memória de nonces nunca esquece: se encher, os pedidos são recusados (`J-E1029`).
- **As pontes** (OPC UA servidor, Modbus servidor, ROS 2) autenticam-se pelo protocolo delas e escrevem como escritores declarados, com nível e lease (documento 01, §14); não levam nonce.
- **Escritas manuais** levam sempre TTL (A10).
- **Nenhuma interface expõe segredos:** um segredo é referido pelo nome (`secret:mqtt_password`), nunca pelo valor.

---

## 10. Limites comuns

| Coisa | Limite v1 |
|---|---|
| segmento de caminho | 32 caracteres; 8 segmentos |
| `string` num sinal | 64 B UTF-8 |
| `bytes` num sinal | 64 B |
| string num evento ou argumento | 1 KiB |
| evento (registo completo, CBOR) | 8 KiB |
| argumentos ou resultado de um método ou ação | 4 KiB |
| mensagem de caixa de correio entre ciclo e async | 256 B |
| `kv` de uma app | chave 128 B; valor 64 KiB |
| buffer de propostas de um `on-cycle` | 16 KiB por omissão (ref/04 §3.3) |
| lista num evento ou argumento | 64 elementos |
| registo interno de app (`Internal<T>`) | 64 B |
| níveis do árbitro | 16 |
| chave de idempotência | 64 B |
| trama do Joker Link | `max_frame` 512–4 096 B (omissão 1 024) |
| mensagem do Joker Link | `max_msg` 2 048–65 535 B (omissão 4 096) |
| corpo de um pedido HTTP | 16 MiB (artefactos vão por upload em blocos, ref/09) |
