# ref/06 — Drivers: trait tier N, mundo `joker:driver` (tier W) e JDP (tier X)

Referência normativa das três formas de escrever um driver. Aplica o `ref/00` (representação) e remete para o documento 16 em tudo o que é semântica. Os ficheiros máquina-legíveis são normativos ao mesmo nível deste documento:

| Ficheiro | Conteúdo |
|---|---|
| `wit/joker-driver.wit` | mundo `joker:driver` completo (tier W) |
| `spec/cddl/jdp.cddl` | mensagens de controlo do JDP, estruturas partilhadas pelos três tiers, descritor, linguagem de esquemas |
| `spec/c/jdp.h` | API C do SDK `libjdp` (tier X) |

---

## 1. Âmbito e fontes

**Cobre:** o contrato entre o núcleo e um driver, igual nos três tiers (§2); a API Rust dos drivers nativos e o que o núcleo faz por cima deles (§3); o mundo WIT dos drivers Wasm, com versões, erros, tipos e limites (§4); o protocolo JDP ao nível do fio, a supervisão, a segurança e os SDKs C e Rust (§5); a suite de conformidade de drivers, a mesma para os três tiers (§6); os códigos J0850–J0899 (§7).

**Não cobre:** as chaves do manifesto (`[buses]`, `[devices]`, `[drivers]`: ref/01), as chaves dos perfis e a linguagem de passos das ações (ref/03), os campos do lock (ref/02), o Joker Link (ref/07), a ABI C do Joker Micro (ref/08).

**Deriva de:** 05 (todo), 01 §2–§4, §7, §8, §11, §17, §20; 02 §4–§8; 09 ataques 3, 4, 7 e 8; 19 §2–§4; 16 §2, §3 e §9; 17 §5.2; 21 §4; ADR 0005, 0007, 0008, 0016, 0020, 0022.

**Liga-se a:**

| Referência | O que partilha com esta |
|---|---|
| ref/01 | `[buses.<b>]`, `[devices.<d>]` (chaves comuns; o anexo A é o esquema `config` de cada driver do catálogo), `[drivers.<n>]` (§5.7 da ref/01 é a lista normativa das chaves; esta referência diz o que cada uma faz, §4.5, §5.2) |
| ref/02 | `[drivers.<n>]` do lock (`tier`, `version`, `digest`, `descriptor`, `wit`, `jdp`) e `[discovered.<d>]` (esquema `discovered`); deriva `J0401` |
| ref/00 | tipos de media dos artefactos (§7.1: `joker.driver.v1+wasm`, `joker.driver.v1+tar`, papel `drivers`), `link_state` (§6), gama de códigos J0850–J0899 (§8) |
| ref/03 | chaves de ponto e de endereço dos perfis (§2.2.4–§2.2.7), `quirks`, `[config]`, `[fallback]`, passos de ações, `[connect]`, `[relinquish]`; códigos J0808–J0826 que o `plan` devolve |
| ref/04 | `joker:runtime/types` (`quality-code`, `instant`, `error`) e `joker:runtime/log`, importados pelo mundo `joker:driver` |
| ref/08 | drivers `no_std` compilados no Joker Micro; o plano entra no blob de geração |
| ref/09 | `/v1/runtime/drivers`, `/v1/devices/<d>` (estado do link, métricas), `jk driver test` e `jk discover` como clientes |

---

## 2. O contrato comum

Um **driver** é código que fala um protocolo. Uma **instância de driver** é esse código ligado a um dispositivo (`instance = device`, o caso comum) ou a um barramento inteiro (`instance = bus`: mestre EtherCAT, mestre CANopen). O contrato é **o mesmo nos três tiers**: as mesmas operações, os mesmos registos, os mesmos códigos. Muda só o transporte da chamada (chamada Rust, chamada Wasm, JDP).

### 2.1 Ciclo de vida

```
                     (jk check, jk build, preparação da geração: sem I/O)
  descriptor ──▶ plan(request) ──▶ read-plan
                                        │
  (jokerd, no domínio io)               ▼
  open(request, plan, link) ──▶ connect() ──▶ ┌── read(batch)        ← escalonador do barramento
         ▲                         │          ├── write(writes)      ← caixas latest-wins, filas ordenadas
         │                         │          ├── read-addr / write-addr  ← passos (ações, connect, fallback)
         │                         │          ├── invoke(call)       ← ações nativas
         │                         │          └── health()           ← a cada 5 s
         │                         ▼
         │                  falha de ligação ──▶ disconnect() ──▶ backoff ──▶ connect()
         │
   geração nova com mudanças neste dispositivo ──▶ disconnect() ──▶ close ──▶ open(…)
```

| Operação | Pura | Pode fazer I/O | Onde corre | Obrigatória |
|---|---|---|---|---|
| `plan` | sim | não | `jk check`, `jk build`, `jk lock`, preparação da geração no nó | sim |
| `open` | não (cria estado) | não | nó, ao ativar a geração | sim |
| `connect` | não | sim | domínio `io` | sim |
| `read` | não | sim | domínio `io` | sim |
| `write` | não | sim | domínio `io` | com a capacidade `write` |
| `read-addr`, `write-addr` | não | sim | domínio `io` | com a capacidade `raw-access` |
| `invoke` | não | sim | domínio `io` | com a capacidade `native-actions` |
| `scan` | não | sim (só leituras) | instância à parte (`jk discover`) | com a capacidade `scan` |
| `health` | sim (lê estado) | não | domínio `io` | sim (há omissão) |
| `disconnect` | não | sim | domínio `io` | sim (há omissão) |

**Sequência de ligação** (a mesma em todos os tiers; é o núcleo que a conduz, e não o driver):

1. `connect()` → `connect-info` por dispositivo (identidade e, com `discover = true`, o mapa descoberto).
2. O núcleo compara identidade, firmware, certificado e mapa com o lock (ref/02 §3.1.5) e gera `J0401` com o `kind` certo. Em `prod`, com deriva, as saídas do dispositivo ficam bloqueadas (05 §12, `drift`).
3. O núcleo executa os passos `[connect]` do perfil com `write-addr`/`read-addr` (ref/03 §2.2.13). Uma falha põe o link em `down`.
4. O núcleo lê os pontos `on-demand` (lotes `on-demand`) e os registos de fallback (`read-addr`).
5. M1: com `fallback.configure = true`, escreve os registos de fallback (`write-addr`), volta a lê-los e compara com o `safe` (`J0205`, `kind = fallback` do `J0401`).
6. O núcleo reenvia o **valor aplicado** de cada saída (`write-kind = resync`), pela ordem dos grupos atómicos (A8 do documento 16: no arranque de uma geração, o aplicado é o `safe`).
7. O link passa a `up` e o escalonador começa os lotes periódicos e arma os lotes `push`.

### 2.2 Referência a um driver e tiers

| Forma em `[devices.<d>] driver` | Tier | Resolução |
|---|---|---|
| `"<nome>"` | N | entrada do registo compilado no `jokerd` (§3.8); ausente no nó: `J0850` |
| `"w:<nome>"` | W | `[drivers.<nome>]` com `tier = "w"` (ref/01 §5.7); componente em `source` |
| `"x:<nome>"` | X | `[drivers.<nome>]` com `tier = "x"`; executável em `exec` ou `source` |

`<nome>` segue `[a-z][a-z0-9-]{0,31}` (ref/01 §3.8). O mesmo nome pode existir em dois tiers (um `bacnet-ip` tier X e, mais tarde, nativo): o prefixo decide, e mudar de tier é uma mudança explícita no manifesto.

### 2.3 Descritor

Cada driver publica um **descritor** estático (`descriptor` em `spec/cddl/jdp.cddl`). No tier N está compilado no binário; no tier W é a secção custom `joker:driver-meta` do componente (CBOR determinístico, legível sem instanciar); no tier X vai no `hello` e, para o `jk check` sem nó, num ficheiro `<exec>.jdp-meta` ao lado do executável ou na camada `descriptor` do artefacto OCI (§5.9).

| Campo (chave) | Tipo | Obrig. | Validação | Uso |
|---|---|---|---|---|
| `name` (0) | nome de driver | sim | igual ao nome em `[drivers]` ou no registo (`J0851`) | |
| `version` (1) | SemVer | sim | igual ao do lock (`J0885`) | |
| `tier` (2) | `n` 0, `w` 1, `x` 2 | sim | igual ao prefixo (`J0851`) | |
| `protocols` (3) | lista de famílias | sim, ≥ 1 | famílias de ref/03 §2.2.2 (`modbus`, `i2c`…) ou `x:<nome>`/`w:<nome>` | o `driver` do perfil tem de estar aqui (`J0808`) |
| `buses` (4) | lista de `bus-kind` | sim, ≥ 1 | | o `kind` do `bus` do dispositivo tem de estar aqui (`J0171`); `none` = sem transporte |
| `instance` (5) | `device` 0, `bus` 1 | sim | | §2.1 |
| `capabilities` (6) | lista de nomes (§2.13) | sim | nomes desconhecidos ignorados | `J0854` |
| `config` (7) | esquema (§2.3.1) | sim (pode ser vazio) | | chaves específicas de `[devices.<d>]` |
| `address` (8) | esquema | sim | | chaves de endereço dos pontos |
| `quirks` (9) | esquema | sim | | `quirks` do perfil |
| `native` (10) | esquema | sim | | tabela `native` das ações do perfil |
| `resources` (11) | `{ memory, cpu, tasks }` | W e X | W: `memory` ≤ 64 MiB; X: sem limite próprio (`J0874` acima do `[drivers]`) | §4.5, §5.7 |
| `sdk` (12) | string | não | | diagnóstico |
| `interface` (13) | `joker:driver@x.y.z`, `jdp/1` ou `joker-hal@x.y` | W, X | versão suportada (`J0859`, `J0861`) | |
| `micro` (14) | bool | não (`false`) | só tier N | compila `no_std` para o Joker Micro (§3.7) |
| `license` (15) | SPDX | não (sim para publicar) | | Hub |
| `description` (16) | string ≤ 1 KiB | não | | |
| `discovered` (17) | esquema | com `discover` | | campos do driver em `[discovered.<d>]` do lock (ref/02 §2.1.11) |

**Os esquemas dos drivers do catálogo** não se inventam por driver: são as tabelas das outras referências, campo a campo. `config` = as chaves do dispositivo do anexo A da ref/01; `address` = as chaves de endereço da família na ref/03 §2.2.5 (e, nos drivers sem perfil, as chaves por sinal do anexo A, com os mesmos nomes); `quirks` = ref/03 §2.2.6; `discovered` = a coluna `[discovered]` do anexo A da ref/01. Um descritor do catálogo que diverja delas é um bug do driver, apanhado por `drv.meta.schema_roundtrip` (§6.4).

**Digest do descritor:** SHA-256 do CBOR determinístico do `descriptor`. O lock fixa-o (`[drivers.<n>] descriptor`, ref/02 §2.1.8), a cache de planos usa-o como chave (§5.3.3), e o blob de geração de um MCU e o `HELLO` do Joker Link levam-no (§3.7).

#### 2.3.1 Linguagem de esquemas

Os esquemas (`field` no CDDL) descrevem chaves TOML que o núcleo valida **antes** do `plan`, com mensagens iguais em todos os drivers. É isto que permite que o `jk check` diga "`unit_id` tem de estar entre 1 e 247" num driver tier X cujo executável não corre no portátil.

| Tipo | TOML aceite | Chega ao driver (CBOR) |
|---|---|---|
| `string` | string (≤ `max` B) | text |
| `int`, `uint`, `float` | número (com `range`) | int, uint, float64 |
| `bool` | bool | bool |
| `duration` | literal de duração (ref/00 §3.1) | uint (ns) |
| `quantity` | literal de quantidade da dimensão de `unit` | float64 na unidade `unit` |
| `choice` | uma das strings de `values` | text |
| `list` | array de `element` (≤ `max`) | array |
| `table` | tabela com os campos de `fields` | map de chaves text |
| `map` | tabela de chaves livres (`snake_case`) com valores `element` | map |
| `address` | `"host:porta"`, IP, ou IP/CIDR | text |
| `path` | caminho absoluto | text |
| `secret` | `"secret:<nome>"` (ref/00 §9) | no `plan`: o nome (text); no `open`: bstr com o valor, nunca registado em log |
| `any` | qualquer valor TOML | CBOR equivalente; valida o `plan` |

Erros: chave desconhecida `J0140` (ref/01), tipo errado, fora de `range`, fora de `values`, obrigatória em falta: `J0852` (dispositivo), `J0853` (endereço), `J0818` (quirks, ref/03), `J0821` (tabela `native`, ref/03).

### 2.4 O que o driver recebe

O núcleo compila o manifesto, o lock e o perfil num **pedido de plano** (`plan-request`). O driver nunca lê TOML nem ficheiros de configuração.

#### 2.4.1 Do manifesto

| Origem (ref/01) | Destino no pedido | Quem usa |
|---|---|---|
| `[buses.<b>]` (todas as chaves) | `bus` (`bus-spec`: nome, `kind`, `params` em CBOR) | o host abre o transporte; o driver só lê os parâmetros (ex.: `baud` para o cálculo do custo) |
| `[devices.<d>]`: `driver`, `bus`, `profile` | resolução do driver, do transporte e do perfil | núcleo |
| `[devices.<d>]`: `poll`, `align`, `signals.*.poll` | `point-spec.poll`; agendamento | escalonador |
| `[devices.<d>]`: `timeout` | `device-spec.timeout-ns`; prazo das chamadas (§2.14) | núcleo e driver |
| `[devices.<d>]`: `retries` | repetições feitas pelo **núcleo** (o driver faz uma tentativa por chamada) | núcleo |
| `[devices.<d>]`: `discover` | `connect` devolve `discovered`; o `plan` recebe o mapa fixado no lock | núcleo e driver |
| `[devices.<d>]`: `fallback` | registos lidos e escritos com `read-addr`/`write-addr` (§2.1, passos 4–5) | núcleo |
| `[devices.<d>]`: `signals.*` (`alias`, `max_age`, `limits`, `stuck_detect`, `deadband`, `scale`, `offset`, `unit`, `record`) | não chegam ao driver | estágio de aquisição |
| `[devices.<d>]`: `signals.*.debounce` | `point-spec.debounce-ns` com a capacidade `debounce`; sem ela, o estágio de aquisição faz o debounce | driver ou núcleo |
| `[devices.<d>]`: `signals.*`, chaves de endereço (`line`, `topic`, `node`… nos drivers sem perfil) | `point-spec.addr` | driver |
| `[devices.<d>]`: todas as outras chaves (`address`, `unit_id`, `node_id`, `position`, `network`, `priority`…) | `device-spec.config`, validada pelo esquema `config` do descritor | driver |
| `[devices.<d>]`: `config = { … }` | validada pelo `[config]` do perfil (ref/03 §2.2.10); os valores sem `apply` vão em `device-spec.profile-config` | driver (*chip handlers*) |
| `[devices.<d>]`: `actions.*` (`permit`, `role`, `timeout`) | não chegam ao driver, só `action-spec.timeout-ns` | núcleo |
| `[outputs."<s>"]`: `timed`, `schedule_depth` | `point-spec.timed`; `committed-write.at` | driver com `timed-write` |
| `[outputs."<s>"]`: `safe = "relinquish"` | `write-kind = relinquish` se o perfil diz `native = true`; senão, passos `[relinquish]` com `write-addr` | driver ou núcleo |
| `[atomic.<g>]` | `committed-write.atomic` e a ordem das escritas numa chamada | driver |
| `[drivers.<n>]` | arranque (W: componente e limites; X: processo, utilizador, sandbox, §5.2.4) | host |

#### 2.4.2 Do perfil

| Origem (ref/03) | Destino | Quem usa |
|---|---|---|
| `[profile] driver` | tem de estar em `descriptor.protocols` (`J0808`) | núcleo |
| `[profile] quirks` | `device-spec.quirks`, validado pelo esquema `quirks` | driver (`plan`) |
| `[profile] firmware` | comparado com `device-info.firmware` (`J0826`) | núcleo |
| `[defaults]` + `[signals]`: chaves de endereço (§2.2.5) | `point-spec.addr` | driver |
| `type` | `point-spec.type-name`, `expect` e `width` (§2.7) | driver e núcleo |
| `direction`, `poll`, `kind` | `point-spec.direction`, `poll` | driver e núcleo |
| `word_order`, `byte_order`, `bit`, `invert`, `scale`, `offset`, `scale_factor`, `unit`, `quantity`, `enum`, `bits`, `invalid`, `limits`, `stuck_detect`, `deadband`, `monotonic`, `len`, `encoding` | não chegam ao driver (exceto `len`, via `width`) | estágio de aquisição |
| `[coherent]` e pares `scale_factor` | `point-spec.coherent` | driver (`plan`) |
| `[config]` | esquema do `config = { … }` do dispositivo | núcleo; driver recebe os valores |
| `[fallback]` | endereços lidos e escritos pelo núcleo com `read-addr`/`write-addr` | núcleo |
| `[actions.<a>] steps` | executados pelo núcleo com `read-addr`/`write-addr`/`sleep` (ref/03 §2.2.12) | núcleo |
| `[actions.<a>] native` | `action-spec` e `invoke` | driver |
| `[connect] steps`, `[relinquish.<s>] steps` | executados pelo núcleo | núcleo |
| `[relinquish.<s>] native = true` | `write-kind = relinquish` | driver com `relinquish` |
| `[events]`, `[enums]`, `[implements]`, `[discovery] names` | não chegam ao driver | núcleo |
| `[discovery] kind`, `bases`, `models`, `max_models` | `device-spec.quirks` (chave `discovery`) | driver com `discover` |

#### 2.4.3 Conversão TOML → CBOR

Os campos `config`, `profile-config`, `quirks`, `addr`, `native` e `params` são o CBOR **determinístico** (RFC 8949 §4.2.1) da tabela TOML, depois de validada e normalizada pelo esquema: chaves text; durações em ns (uint); quantidades em float64 na unidade do esquema; inteiros como int/uint; datas como text RFC 3339; valores `any` convertidos tipo a tipo. Duas tabelas iguais dão sempre os mesmos bytes, e é isso que torna o `plan` comparável entre execuções e entre máquinas.

### 2.5 Pontos (`point-spec`)

| Campo | Tipo | Significado |
|---|---|---|
| `id` | u32 | índice denso na instância (0..n), por ordem do caminho canónico; estável dentro de uma geração |
| `device` | u16 | índice do dispositivo na instância (0 em `instance = device`) |
| `path` | string | caminho relativo ao dispositivo, só para diagnóstico |
| `direction` | `input` \| `output` \| `param` | |
| `expect` | `raw-kind` | forma crua que o núcleo espera (§2.7) |
| `width` | u16 | registos (`regs`), bytes máximos (`bytes`, `text`) ou 0 |
| `type-name` | string | `type` do perfil, tal qual |
| `addr` | CBOR | endereço, no esquema `address` do driver |
| `poll` | `period(ns)` \| `on-demand` \| `push` | |
| `class` | `cycle` \| `telemetry` \| `diagnostic` | `cycle` se algum grupo de ciclo lê o sinal; `diagnostic` para `joker.*` e parâmetros; senão `telemetry` |
| `coherent` | u32 opcional | grupo de coerência (grupos que partilham pontos já fundidos, ref/03 §2.2.9) |
| `debounce-ns` | u64 | §2.4.1 |
| `timed` | bool | saída com escritas temporizadas |

As saídas também são pontos (`direction = output`) e **não são lidas em ciclo**: o slot de uma saída tem o valor aplicado (ref/03 §2.2.5). Um lote nunca contém uma saída.

### 2.6 Plano (`read-plan`)

O `plan` agrupa os pontos em **lotes** (`batch-spec`). Cada lote é uma operação do protocolo (um pedido Modbus, um `Read` OPC UA, uma subscrição, um PDO).

| Campo | Regra | Violação |
|---|---|---|
| `batches[].id` | 0..n−1, pela ordem da lista | `J0856` |
| `batches[].points` | crescentes, sem repetidos, todos do mesmo `device`, nenhum `output` | `J0856` |
| cobertura | cada ponto `input`/`param` está em **exatamente um** lote | `J0856` |
| `kind` | `poll` só com pontos `period`, e todos com o **mesmo** período; `on-demand` só com pontos `on-demand`; `push` só com pontos `push` | `J0856` |
| `period-ns` | igual ao período dos pontos (`poll`), 0 nos outros | `J0856` |
| coerência | os pontos com o mesmo `coherent` estão no mesmo lote | se não couberem: `J0813` (ref/03); se o driver os separar: `J0856` |
| `cost-ns` | > 0 em `poll` e `on-demand` nos barramentos com escalonador | entra no orçamento `J0370` (05 §5) |
| `write-cost-ns` | custo de uma escrita de um ponto | idem |
| `private` | ≤ 4 KiB por lote, ≤ 1 MiB no plano | `J0856` |
| `warnings` | só avisos | mostrados pelo `jk check` com o código |
| erros | `list<diagnostic>`, com código de ref/03 §2.9 (`J0809`–`J0813`, `J0818`, `J0821`) ou desta referência; outro código ou nenhum: `J0855` | |

O plano é **determinístico**: o mesmo pedido (os mesmos bytes CBOR) dá o mesmo plano (os mesmos bytes CBOR). O núcleo guarda o plano na geração e, no Joker Micro, no blob (§3.7). A preparação da geração no nó recalcula-o e compara-o com o do `jk check`; diferentes é `J0857`.

### 2.7 Valores crus e codificação

O driver entrega e recebe **valores crus**. O núcleo converte entre cru e valor de engenharia num só sítio (01 §3.3; ref/03 §3 "Aquisição").

| `raw-kind` | Código | Payload | Usado por |
|---|---|---|---|
| `none` | 0 | — | só em falhas; com `status` `good` é `J0869` |
| `boolean` | 1 | bool | coils, discrete inputs, GPIO |
| `int` | 2 | i64 | protocolos já tipados (OPC UA Int32, CANopen INTEGER) |
| `uint` | 3 | u64 | idem, e campos de tramas CAN |
| `float` | 4 | f64 | OPC UA Float/Double, MQTT JSON |
| `regs` | 5 | `width` palavras de 16 bits, pela ordem do protocolo | Modbus (registos de 16 bits) |
| `bytes` | 6 | ≤ 256 B | I²C, SPI, M-Bus, protocolos binários |
| `text` | 7 | UTF-8 ≤ 256 B | protocolos de texto, MQTT |

**`expect` e `width` a partir do `type` do perfil:**

| Família | `type` | `expect` | `width` |
|---|---|---|---|
| `modbus` (FC 1, 2, 5, 15) | `bool` | `boolean` | 0 |
| `modbus` (FC 3, 4, 6, 16) | `bool` (com `bit`), `u16`, `i16`, `bits16` | `regs` | 1 |
| | `u32`, `i32`, `f32`, `bits32` | `regs` | 2 |
| | `u64`, `i64`, `f64`, `bits64` | `regs` | 4 |
| | `string`, `bytes` | `regs` | `len` |
| `i2c`, `spi` | qualquer | `bytes` | `len` (ou o tamanho do tipo) |
| `gpio`, `revpi` (bits) | `bool` | `boolean` | 0 |
| outras famílias e `x:`/`w:` | numéricos | `int`, `uint` ou `float`, o que o protocolo der | 0 |
| | `string` | `text` | `len` |
| | `bytes` | `bytes` | `len` |

O descritor não declara esta tabela: o `plan` recusa um `type` que o driver não saiba entregar (`J0809`). Uma amostra com um `raw-kind` diferente de `expect` é `J0869` e fica `bad(config_error)`.

**Leitura (cru → valor):** a ordem de ref/03 §3 (descodificação com `word_order`/`byte_order`/`bit`/`invert` → `invalid` → escala → unidade → NaN → `limits` → `enum` → `deadband` → `stuck_detect` → link). Um `int`/`uint`/`float` já tipado salta a descodificação.

**Escrita (valor → cru):** o inverso, feito pelo núcleo depois do envelope: unidade do sinal → unidade do perfil → `(v − offset) / scale` → arredondamento para o inteiro mais próximo, com empates para o par (`round_ties_even`, igual em todas as arquiteturas) → verificação da gama do tipo cru → codificação (`word_order`, `byte_order`). Um valor fora da gama do tipo cru nunca se satura em silêncio: o `jk check` recusa um `safe` não representável (`J0858`, erro) e avisa de uma `range` não representável (`J0858`, aviso); em runtime, uma escrita não representável é `Rejected(Range)` no registo de decisões e não chega ao driver.

### 2.8 Amostras e qualidade

`raw-sample { point, value, status, ts-src, ts-hw }`. O `status` é a qualidade que **o equipamento** reporta. As qualidades que dependem do tempo, dos limites, do envelope ou do ML são do núcleo.

| Código | Motivos que um driver pode reportar | Exemplo |
|---|---|---|
| `good` | `none`, `overridden`, `local_override` | OPC UA `Good_LocalOverride`; PLC com o valor forçado |
| `uncertain` | `stale`, `substituted`, `sensor_cal`, `initial`, `clock_unsynced` | OPC UA `Uncertain_LastUsableValue`, `Uncertain_SubstituteValue`, `Uncertain_InitialValue`; carimbo `ts-src` com relógio remoto não sincronizado |
| `bad` | `comm_failure`, `device_fault`, `out_of_range`, `config_error`, `not_connected`, `stale` | ponto com erro próprio numa resposta em lote (OPC UA `Bad_*` por nó) |
| qualquer | 240–255 (privados, ref/00 §4.3) | o código conta; fora do nó, o motivo aparece como 0 |

**Só o núcleo** gera `not_confirmed`, `out_of_distribution`, `frozen`, `partial_write`, `nan`, `tripped` e `deadline`. Um par fora da tabela, um ponto fora do lote, um segundo valor para o mesmo ponto na mesma leitura, ou um `none` com `good`, é `J0869` (aviso, contado em `joker_driver_invalid_samples_total{device}`), e a amostra passa a `bad(config_error)`. Um ponto do lote sem amostra fica `bad(comm_failure)` nessa leitura.

**Falhas de lote** (`batch-failure`): o núcleo aplica-as a todos os pontos do lote.

| Variante | Código JDP | Qualidade dos pontos | Conta para o link |
|---|---|---|---|
| `timeout` | 0 | `bad(comm_failure)` | sim |
| `comm-failure(detalhe)` | 1 | `bad(comm_failure)` | sim |
| `device-fault(detalhe)` | 2 | `bad(device_fault)` | não (o equipamento respondeu) |
| `not-connected` | 3 | `bad(comm_failure)` | passa o link a `down` |
| `config-error(detalhe)` | 4 | `bad(config_error)` | não; aviso `J0869` |

**Qualidade que vem do link** (05 §12), aplicada pelo núcleo por cima da do driver: `connecting` → `uncertain(initial)` nos pontos sem valor; `degraded` → `uncertain(stale)` nos lotes afetados; `down` → `bad(comm_failure)`; `stopped` (driver morto, em trap ou em crashloop) → `bad(not_connected)`; `drift` → valores lidos normalmente, saídas bloqueadas em `prod`.

**Tempo:** `ts-src` é UTC em ns do equipamento; `ts-hw` é monotónico do nó (o mesmo `CLOCK_MONOTONIC` para o `jokerd` e para um processo tier X no mesmo nó). O `ts_acq` é posto pelo núcleo, e todas as amostras de uma chamada `read` (ou de um mesmo lote `push` entregue de uma vez) recebem o mesmo `acq_id` (02 §6.2).

### 2.9 Escritas

O driver só recebe **escritas aplicadas** (ADR 0005). Nunca vê propostas.

| `write-kind` | Código | Origem | O driver |
|---|---|---|---|
| `set` | 0 | commit de um grupo (caixa latest-wins ou fila ordenada) | escreve |
| `keepalive` | 1 | renovação do fallback do equipamento (01 §8); nunca suprimida (E8) | escreve, mesmo valor |
| `force-safe` | 2 | bit `force_safe` do supervisor (01 §3.4, §20) | escreve; numa saída com adaptador `edge`, o núcleo converte-o nos passos da ação `fall` (17 §5.2) |
| `relinquish` | 3 | `safe = "relinquish"` com `[relinquish.<s>] native = true` | liberta (valor `none`) |
| `resync` | 4 | reenvio do valor aplicado depois de cada `connect` (§2.1, passo 6) | escreve |

`committed-write { seq, point, value, kind, at, atomic }`:

- **Ordem:** uma chamada `write` leva uma ou mais escritas, pela ordem em que têm de ir para o fio. O driver respeita-a. As escritas de um grupo atómico (M20) vão **sempre juntas numa só chamada**, pela `order` declarada (`atomic.index` 0..count−1), e o driver não intercala nelas outras escritas do mesmo dispositivo. Com a capacidade `atomic-request`, o driver põe-nas no mesmo pedido quando o protocolo o permite (FC 16 contíguo).
- **Acks:** exatamente um `write-ack` por escrita, pela mesma ordem, com o mesmo `seq`. Se uma escrita de um grupo atómico falha, as seguintes do grupo levam `skipped` e o núcleo marca o grupo `uncertain(partial_write)` (02 §7.3).
- **Temporizadas:** `at` só aparece com a capacidade `timed-write` e em pontos `timed`. O driver aplica a escrita em `at` com a sua precisão declarada; se não conseguir até `at` + tolerância (a do descritor, 100 µs por omissão no GPIO), devolve `late`, e o núcleo regista `J0878`. Sem a capacidade, o núcleo aplica a escrita no primeiro tick com `t ≥ at` e o driver recebe-a sem `at` (02 §7.2).
- **Impulsos** (`pulse`): o núcleo gera duas escritas `set` (o valor e, depois, o `safe`). O driver não sabe o que é um impulso.

| `write-status` | Código | Efeito no núcleo |
|---|---|---|
| `ok` | 0 | escrita confirmada no registo de decisões |
| `timeout` | 1 | conta para o link; N timeouts seguidos → `down` (01 §20) |
| `comm-failure` | 2 | idem |
| `device-rejected` | 3 | `J0877` no registo de decisões; a saída fica `uncertain(not_confirmed)` até ao readback |
| `not-connected` | 4 | link `down`; a caixa latest-wins mantém o valor para o `resync` |
| `unsupported` | 5 | `J0877`; em escritas `relinquish` sem suporte, o `jk check` já devia ter dado `J0207` |
| `late` | 6 | `J0878` |
| `skipped` | 7 | grupo atómico parcial |

### 2.10 Acesso por endereço, passos e ações

A linguagem de passos dos perfis (ref/03 §2.2.12: `write`, `read`, `wait`, `until`, `fail_if`) é executada **pelo núcleo**, e não pelo driver. O driver só oferece duas primitivas (capacidade `raw-access`):

- `read-addr { device, addr, expect, width, type-name } → raw-value | batch-failure`;
- `write-addr { device, addr, value, type-name } → () | batch-failure`.

O endereço vem já em CBOR no esquema `address` do driver, e o valor já codificado (§2.7; um passo com `signal = "<saída>"` é escalado pelo núcleo). Passam pela fila serializada do dispositivo e pelo escalonador do barramento, com a prioridade das escritas. Os passos servem as ações de perfil, `[connect]`, `[relinquish]`, `[config] apply` e o fallback (M1). Assim, um driver novo tem ações, fallback e relinquish **sem escrever código para isso**.

As **ações nativas** (`[actions.<a>] native = { … }`: um método OPC UA, um SDO CANopen, um comando de fabricante) chegam a `invoke(action-call { call, action, args })`:

- `args` é o CBOR determinístico do registo de argumentos, já validado pelo núcleo contra o `args` da ação (`J0822`); o resultado é o CBOR do registo `result`;
- o permissivo, o papel, a serialização por dispositivo, a chave de idempotência e a `dedup_window` (E9) são do núcleo: uma invocação repetida dentro da janela nunca chega ao driver;
- ações longas (`long = true`) reportam progresso (`progress(call, permille, texto)`) e verificam o cancelamento (`cancelled(call)`); o resultado final é o de `invoke`;
- `action-failure`: `timeout`, `device(error)`, `invalid(error)`, `not-connected`, `unsupported`, `cancelled`. O núcleo projeta-as no `action-error` das apps (ref/04): `device` → `device`, `invalid` → `invalid`, `timeout` → `timeout`, os outros → `device` com o código.

### 2.11 Descoberta e scan

- **Descoberta de um dispositivo** (`discover = true`, M2): corre dentro de `connect`, e o mapa vai em `connect-info.discovered`. O núcleo calcula o `digest` (SHA-256 do CBOR determinístico, depois de o **recodificar** ele próprio) e compara com o lock. O `plan` recebe sempre o mapa **do lock** (`device-spec.discovered`); em `prod` um mapa não fixado não é usado (05 §7). Os campos comuns de `[discovered.<d>]` são os de `device-info` (`vendor`, `model`, `serial`, `firmware`, `hw_revision`, `cert`), e os campos do driver, tirados de `device-info.extra` e do mapa descoberto, seguem o esquema `discovered` do descritor (ref/02 §2.1.11).
- **Scan de rede** (`jk discover <uri>`, capacidade `scan`): numa instância própria, com um link criado a partir do URI e das opções. Devolve `scan-hit { address, config, info, profile }`. O host limita o ritmo (por omissão 20 pedidos/s) e o prazo (`--timeout`, 60 s).
- **Só leituras.** Nem a descoberta nem o scan escrevem no equipamento. O host não chama `write`, `write-addr` nem `invoke` nessas fases; o que o driver envia pelo transporte é da sua responsabilidade, e a conformidade verifica-o com um duplo que regista escritas (`drv.discover.read_only`, §6). Uma escrita detetada é `J0871`.

### 2.12 Estado do link e saúde

**Enumeração `link_state`** (ref/00 §6): `connecting` 0, `up` 1, `degraded` 2, `down` 3, `drift` 4, `stopped` 5. O driver só reporta os quatro primeiros (é o `link-state` do WIT e do CDDL); `drift` (deriva face ao lock) e `stopped` (driver não está a correr) são do núcleo, e aparecem na API e no relatório ao Deck com os seis valores.

O **estado efetivo** é o pior entre o que o driver reporta (`report-link`) e o que o núcleo calcula das chamadas:

| Estado do núcleo | Entra quando | Sai quando |
|---|---|---|
| `connecting` | antes do primeiro `connect` bem-sucedido; durante o backoff | `connect` e passos 2–6 de §2.1 concluídos → `up` |
| `degraded` | ≥ 20% das últimas 20 execuções de lotes falharam com `timeout` ou `comm-failure` | < 5% nas últimas 20 → `up` |
| `down` | `connect` falha; 3 execuções seguidas de **todos** os lotes falham; `not-connected`; `write-status` `timeout` 3 vezes seguidas | `disconnect` e novo `connect` com backoff 250 ms → 30 s, ×2, jitter ±20% |
| `stopped` | processo X morto, trap W, crashloop; `jk device stop` (`POST /v1/devices/{d}/stop`, ref/09 §5.12: fecho ordenado, sem reinício) | reinício do driver (§5.7); depois de um `stop`, só `jk device start`, o próximo arranque do `jokerd` ou uma geração que mexa no dispositivo |

As transições geram os alarmes `joker.device.<d>.degraded`, `.offline`, `.drift` (05 §12) e o evento `joker.device.<d>.link` com o estado anterior, o novo e o detalhe.

**Saúde:** `health()` devolve `{ link, detail, counters }`. É síncrona, sem I/O, e o núcleo chama-a a cada 5 s. Os contadores (≤ 64, nomes `snake_case`) são exportados como `joker_driver_<nome>{device}`. O núcleo mede ele próprio, para todos os tiers, a latência de cada chamada (p50/p99 por lote), timeouts, reconexões, bytes pelo transporte do host e a eficiência de coalescência (05 §12).

**Crashloop por tier.** Esta tabela é a única fonte dos limiares; o §4.5 e o §5.7 remetem para ela.

| Tier | Conta como falha | Limiar de crashloop | Recuperação | Código e alarme |
|---|---|---|---|---|
| N | não se aplica: corre dentro do `jokerd` | — | — | — |
| W | trap da instância (`Store`), incluindo limite de recursos (`J0868`) | **3 traps em 10 s** (janela deslizante) | recriação com backoff 1 s → 5 min (×2); em crashloop, ao ritmo máximo | `J0866`; `joker.driver.<n>.crashloop` (crítico) |
| X | morte do processo (§5.7, estado `dead`) | **5 mortes em 10 min** | backoff 1 s × 2ⁿ (máx. 5 min, jitter ±20%); 10 min seguidos em `ready` → contador a zero | `J0866`; `joker.driver.<n>.crashloop` (crítico) |

**Porque diferem.** Recriar uma instância W é barato (um `Store` novo, sem processo, em milissegundos), e um trap repete-se logo se a causa é determinista: três em 10 s já dizem que é um bug, e não uma falha passageira. Um processo X demora segundos a arrancar (unidade systemd, handshake, `open` e `connect` das instâncias), e morre também por causas externas (OOM, um SDK de fabricante que perde a ligação): com o backoff, 10 s nem sequer dão para três mortes, e por isso a janela é de 10 min.

### 2.13 Capacidades

| Capacidade | O driver oferece | Exigida por | Sem ela |
|---|---|---|---|
| `write` | `write` | pontos `output` no perfil ou no manifesto | `J0854` |
| `push` | lotes `push` | `poll = push` (subscrições, GPIO por flanco) | `J0854` |
| `raw-access` | `read-addr`, `write-addr` | ações `steps`, `[connect]`, `[relinquish]` com passos, `[config] apply`, `[fallback]` | `J0854` |
| `native-actions` | `invoke` | ações `native` | `J0854` |
| `discover` | mapa descoberto em `connect` | `discover = true` | `J0177` (ref/01) |
| `scan` | `scan` | `jk discover` | o `jk discover` diz que o driver não faz scan |
| `coherent` | leitura coerente num pedido (a mesma resposta) | `[coherent]` e `scale_factor` | `J0854` |
| `atomic-request` | grupo atómico num só pedido quando contíguo | — (melhora a coerência; sem ela, escritas seguidas sem intercalação) | — |
| `timed-write` | `committed-write.at` | `timed = true` | `J0854` |
| `relinquish` | `write-kind = relinquish` | `[relinquish.<s>] native = true` | `J0207` (ref/01) |
| `debounce` | debounce no hardware ou no kernel | — | o estágio de aquisição faz o debounce |
| `hw-timestamps` | `ts-hw` nas amostras | — (informativa: o `jk check` avisa se uma app usa `write_at` sobre `ts_hw` de um sinal sem ela) | — |
| `rt-cycle` | parte de tempo real na thread `rt/drv/<bus>` (§3.5) | barramentos com `cycle` (EtherCAT, piControl com sync) | `J0854` |
| `block-read` | blocos (M7) | `kind = "block"` | reservado 1.1 (`J0807`) |
| `streams` | streams (M4) | `kind = "stream"` | reservado 1.1 (`J0807`) |

A leitura (`read`) não é uma capacidade: todos os drivers a têm.

### 2.14 Concorrência e prazos

- **Uma chamada de cada vez por instância.** O núcleo nunca chama duas operações da mesma instância ao mesmo tempo, com uma exceção: uma `read` pendente de um lote `push` é **cancelada** para correr uma `write`, `read-addr`, `write-addr` ou `invoke`, e é reposta depois. Por isso a leitura de um lote `push` tem de ser *cancel-safe*: cancelada, não perde dados (ficam no estado do driver ou no transporte).
- **Um barramento de cada vez.** O escalonador do barramento (05 §5) serializa as instâncias que partilham um barramento `serial`, `i2c`, `spi` ou `ethernet` com `bus` declarado: só uma chamada está em curso no barramento. Prioridades: escritas e ações → lotes `cycle` por período → `telemetry` → `diagnostic`, descoberta e scan. Os dispositivos TCP sem `bus` e os barramentos `can` não serializam entre dispositivos (cada um tem a sua ligação ou as tramas são assíncronas).
- **Prazos.** Cada chamada tem um prazo absoluto (monotónico) que o driver recebe e deve usar nos timeouts do transporte. Se o driver não devolver até ao prazo mais uma margem, o núcleo cancela a chamada (N e W: o future é largado; X: `CANCEL` no anel e depois morte do processo se nada vier, §5.7), conta `J0867` e trata-a como `timeout`.

| Chamada | Prazo por omissão | Origem |
|---|---|---|
| `plan` | W: 2 s de relógio e 10⁹ unidades de *fuel*; X: 5 s; N: sem prazo (deve ser pura e rápida) | este documento |
| `open` | 1 s | |
| `connect` | `quirks.connect_timeout` (ref/03, 5 s) × n.º de dispositivos da instância, até 60 s | ref/03 §2.2.6 |
| `read` | `timeout` do dispositivo | ref/01 |
| `write` | `timeout` × (1 + ⌊n/8⌋) para n escritas | |
| `read-addr`, `write-addr` | `timeout` por passo | |
| `invoke` | `timeout` da ação | ref/03, ref/01 |
| `scan` | `--timeout` (60 s) | |
| `health` | 100 ms (N, W); 2 s de resposta (X) | |
| `disconnect` | 2 s | |
| margem de cancelamento | 50 ms (N, W); 500 ms (X) | |

As repetições (`retries`) são do núcleo, no mesmo período de poll, se o tempo restante couber: uma tentativa por chamada, sempre.

### 2.15 Limites

| Coisa | Limite v1 | Acima |
|---|---|---|
| pontos por instância | 65 536 | `J0856` |
| lotes por plano | 16 384 | `J0856` |
| dispositivos por instância (`instance = bus`) | 1 024 | `J0856` |
| ações nativas por instância | 4 096 | `J0856` |
| valor cru (`regs`, `bytes`, `text`) | 256 B (depois de descodificado, os sinais continuam limitados a 64 B, ref/00 §10) | `J0869` |
| `config`, `quirks`, `profile-config` (CBOR) | 64 KiB cada | `J0852` |
| `addr` de um ponto | 1 KiB | `J0853` |
| `private` | 4 KiB por lote; 1 MiB por plano | `J0856` |
| plano inteiro (CBOR) | 16 MiB | `J0856` |
| argumentos e resultado de ação | 16 KiB (strings ≤ 1 KiB, listas ≤ 64, ref/00 §10) | `J0822` / `invalid` |
| mensagem de log | 1 KiB, 16 campos; 100/s por instância, rajada 1 000 | cortado, contado (`J0886`) |
| contadores de saúde | 64 | ignorados os excedentes |
| resultados de um scan | 4 096 | cortado |
| amostras por leitura | as do lote | `J0869` |

---

## 3. Tier N: drivers nativos em Rust

### 3.1 Crates

| Crate | `no_std` | Conteúdo |
|---|---|---|
| `joker-hal` | sim (features `alloc`, `std`, `plan`) | tipos do §2, traits `Planner`, `Driver`, `RtPart`, transportes, `Cx`, registo, `MaybeSend`, CBOR (`minicbor`) |
| `joker-drivers/<nome>` | conforme o driver (`micro = true` exige `no_std`) | um driver; exporta uma `DriverEntry` |
| `jokerd` (módulo `drivers`) | não | o registo gerado a partir das features (§3.8), o escalonador de barramentos, o estágio de aquisição, os hosts W e X |
| `joker-jdp` | não | JDP: protocolo, anéis, cliente (lado `jokerd`) e servidor (SDK Rust do tier X, §5.10) |
| `joker-driver-sdk` | sim | SDK de drivers tier W em Rust: `export_driver!` sobre os mesmos traits (§4.6) |
| `joker-drvtest` | não | suite de conformidade e duplos de equipamento (§6) |

**O mesmo código Rust serve os três tiers.** Um driver escrito contra `joker-hal` compila para dentro do `jokerd` (N), para um componente com `joker-driver-sdk` (W), ou para um executável com `joker-jdp` (X). O que muda é o host dos transportes e das chamadas.

### 3.2 Tipos

```rust
// crate joker-hal — #![no_std]; `alloc` e `std` são features.
pub type PointId = u32;
pub type BatchId = u32;
pub type DeviceIx = u16;
pub type MonoNs = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QualityCode { Good = 0, Uncertain = 1, Bad = 2 }

/// Numeric reason of ref/00 §4.3 (240–255 private to the driver).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reason(pub u8);
impl Reason {
    pub const NONE: Self = Self(0);
    pub const OVERRIDDEN: Self = Self(1);
    pub const LOCAL_OVERRIDE: Self = Self(2);
    pub const STALE: Self = Self(3);
    pub const SUBSTITUTED: Self = Self(5);
    pub const SENSOR_CAL: Self = Self(7);
    pub const CLOCK_UNSYNCED: Self = Self(8);
    pub const INITIAL: Self = Self(9);
    pub const COMM_FAILURE: Self = Self(12);
    pub const DEVICE_FAULT: Self = Self(13);
    pub const OUT_OF_RANGE: Self = Self(14);
    pub const CONFIG_ERROR: Self = Self(15);
    pub const NOT_CONNECTED: Self = Self(16);
    pub const fn private(n: u8) -> Self { Self(240 + (n % 16)) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawStatus { pub code: QualityCode, pub reason: Reason }
impl RawStatus {
    pub const OK: Self = Self { code: QualityCode::Good, reason: Reason::NONE };
    pub const fn bad(reason: Reason) -> Self { Self { code: QualityCode::Bad, reason } }
    pub const fn uncertain(reason: Reason) -> Self { Self { code: QualityCode::Uncertain, reason } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RawKind { None = 0, Bool = 1, Int = 2, Uint = 3, Float = 4, Regs = 5, Bytes = 6, Text = 7 }

/// Borrowed raw value; the sink copies it. No allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawValue<'a> {
    None,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Regs(&'a [u16]),
    Bytes(&'a [u8]),
    Text(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub struct RawSample<'a> {
    pub point: PointId,
    pub value: RawValue<'a>,
    pub status: RawStatus,
    pub ts_src: Option<i64>,
    pub ts_hw: Option<MonoNs>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState { Connecting, Up, Degraded, Down }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { Input, Output, Param }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll { Period(u64), OnDemand, Push }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadClass { Cycle, Telemetry, Diagnostic }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchKind { Poll, OnDemand, Push }

/// Zero-copy views: on Linux they borrow the generation; on the Joker Micro, flash.
pub struct PointSpec<'a> {
    pub id: PointId,
    pub device: DeviceIx,
    pub path: &'a str,
    pub direction: Direction,
    pub expect: RawKind,
    pub width: u16,
    pub type_name: &'a str,
    pub addr: Cbor<'a>,
    pub poll: Poll,
    pub class: ReadClass,
    pub coherent: Option<u32>,
    pub debounce_ns: u64,
    pub timed: bool,
}

pub struct ActionSpec<'a> {
    pub id: u32,
    pub device: DeviceIx,
    pub name: &'a str,
    pub native: Cbor<'a>,
    pub timeout_ns: u64,
    pub long: bool,
}

pub struct DeviceSpec<'a> {
    pub index: DeviceIx,
    pub name: &'a str,
    pub config: Cbor<'a>,
    pub profile_config: Cbor<'a>,
    pub quirks: Cbor<'a>,
    pub discovered: Option<Cbor<'a>>,
    pub timeout_ns: u64,
}

pub struct BusSpec<'a> { pub name: &'a str, pub kind: BusKind, pub params: Cbor<'a> }

pub struct PlanRequest<'a> {
    pub bus: Option<BusSpec<'a>>,
    pub devices: &'a [DeviceSpec<'a>],
    pub points: &'a [PointSpec<'a>],
    pub actions: &'a [ActionSpec<'a>],
}

/// A batch at runtime, as handed to `read`.
pub struct ReadBatch<'a> {
    pub id: BatchId,
    pub device: DeviceIx,
    pub kind: BatchKind,
    pub points: &'a [PointId],
    pub private: &'a [u8],
    pub deadline: MonoNs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind { Set, Keepalive, ForceSafe, Relinquish, Resync }

#[derive(Clone, Copy, Debug)]
pub struct AtomicRef { pub group: u32, pub index: u16, pub count: u16 }

pub struct CommittedWrite<'a> {
    pub seq: u64,
    pub point: PointId,
    pub value: RawValue<'a>,
    pub kind: WriteKind,
    pub at: Option<MonoNs>,
    pub atomic: Option<AtomicRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WriteStatus {
    Ok = 0, Timeout = 1, CommFailure = 2, DeviceRejected = 3,
    NotConnected = 4, Unsupported = 5, Late = 6, Skipped = 7,
}

/// Batch-level failure (§2.8). `detail` is for humans, ≤ 40 bytes on the JDP wire.
#[derive(Clone, Copy, Debug)]
pub enum BatchFailure {
    Timeout,
    CommFailure(&'static str),
    DeviceFault(&'static str),
    NotConnected,
    ConfigError(&'static str),
}

/// Error with a registry code (ref/00 §8). No allocation: static strings, or a
/// fixed-capacity buffer with the `DetailBuf` helper.
#[derive(Clone, Debug)]
pub struct DriverError { pub code: Code, pub detail: Detail }

#[derive(Clone, Debug)]
pub enum ActionFailure {
    Timeout,
    Device(DriverError),
    Invalid(DriverError),
    NotConnected,
    Unsupported,
    Cancelled,
}

pub struct DeviceInfo<'a> {
    pub vendor: Option<&'a str>,
    pub model: Option<&'a str>,
    pub serial: Option<&'a str>,
    pub firmware: Option<&'a str>,
    pub hw_revision: Option<&'a str>,
    pub cert_sha256: Option<[u8; 32]>,
    pub extra: Cbor<'a>,
}

pub struct Diagnostic<'a> { pub code: Code, pub point: Option<PointId>, pub message: &'a str }
```

Os nomes, a ordem dos campos e os valores numéricos são os do WIT (§4) e do CDDL. `Cbor<'a>` é uma vista sobre CBOR determinístico com `decode::<T>()` (via `minicbor`, sem alocação quando `T` só pede emprestado). `Code` é um código do registo validado em compilação (`code!("J0810")`).

### 3.3 Traits

```rust
/// Marker: `Send` on std hosts (tokio multi-thread), nothing on the Joker Micro (Embassy).
#[cfg(feature = "std")]
pub trait MaybeSend: Send {}
#[cfg(feature = "std")]
impl<T: Send> MaybeSend for T {}
#[cfg(not(feature = "std"))]
pub trait MaybeSend {}
#[cfg(not(feature = "std"))]
impl<T> MaybeSend for T {}

/// Planning half. Host-side only (jk, jokerd, jk build for MCUs): needs `alloc`.
#[cfg(feature = "plan")]
pub trait Planner {
    /// Static descriptor (§2.3).
    const DESCRIPTOR: &'static Descriptor;

    /// Pure: no I/O, no clock, no randomness, no global state. Same request → same plan.
    fn plan(req: &PlanRequest<'_>) -> Result<Plan, Vec<Diagnostic<'static>>>;
}

/// Runtime half. `no_std`, no allocation required.
pub trait Driver: Sized + MaybeSend + 'static {
    /// Builds the instance state. No I/O. `args` gives the request, the plan and the link.
    fn open(args: &OpenArgs<'_>, link: Link) -> Result<Self, DriverError>;

    fn connect(
        &mut self,
        cx: &mut Cx<'_>,
        out: &mut ConnectOut<'_>,
    ) -> impl Future<Output = Result<(), DriverError>> + MaybeSend;

    /// One execution of `batch`; one `out.push` per point. For `Push` batches: return as soon
    /// as at least one sample was pushed, or `Ok(())` at the deadline. MUST be cancel-safe for
    /// `Push` batches.
    fn read(
        &mut self,
        cx: &mut Cx<'_>,
        batch: &ReadBatch<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend;

    /// Capability `write`. One `acks.set(i, …)` per write; unset acks become `CommFailure`.
    fn write(
        &mut self,
        cx: &mut Cx<'_>,
        writes: &[CommittedWrite<'_>],
        acks: &mut WriteAcks<'_>,
    ) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = (cx, writes);
            acks.fill(WriteStatus::Unsupported);
        }
    }

    /// Capability `raw-access`.
    fn read_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrRead<'_>,
        out: &mut RawSink<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req, out);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    fn write_addr(
        &mut self,
        cx: &mut Cx<'_>,
        req: &AddrWrite<'_>,
    ) -> impl Future<Output = Result<(), BatchFailure>> + MaybeSend {
        async move {
            let _ = (cx, req);
            Err(BatchFailure::ConfigError("raw-access not supported"))
        }
    }

    /// Capability `native-actions`. The result record goes to `out` as CBOR.
    fn invoke(
        &mut self,
        cx: &mut Cx<'_>,
        call: &ActionCall<'_>,
        out: &mut ActionOut<'_>,
    ) -> impl Future<Output = Result<(), ActionFailure>> + MaybeSend {
        async move {
            let _ = (cx, call, out);
            Err(ActionFailure::Unsupported)
        }
    }

    /// Synchronous, cheap, no I/O.
    fn health(&self, out: &mut HealthOut<'_>) {
        let _ = out;
    }

    /// Must succeed.
    fn disconnect(&mut self, cx: &mut Cx<'_>) -> impl Future<Output = ()> + MaybeSend {
        async move {
            let _ = cx;
        }
    }
}

/// Capability `scan`: runs in its own instance for `jk discover`.
pub trait Scanner: MaybeSend + 'static {
    fn scan(
        cx: &mut Cx<'_>,
        req: &ScanRequest<'_>,
        link: Link,
        out: &mut ScanOut<'_>,
    ) -> impl Future<Output = Result<(), DriverError>> + MaybeSend;
}

/// Capability `rt-cycle`: the part of the driver that runs on `rt/drv/<bus>` (§3.5).
pub trait RtCapable: Driver {
    type Rt: RtPart;
    /// Called once after `connect`; the core moves the part to the RT thread.
    fn rt_part(&mut self) -> Result<Self::Rt, DriverError>;
}

pub trait RtPart: Send + 'static {
    /// One bus cycle. Synchronous, no allocation, no blocking, no await, bounded WCET.
    fn cycle(&mut self, io: &mut RtIo<'_>) -> RtStatus;
}
```

- A implementação escreve `async fn read(…)`; o compilador verifica que o future é `Send` no Linux (o método do trait declara `impl Future + MaybeSend`).
- `OpenArgs` dá `request()`, `plan()` (`PlanView`: lotes, `private` de cada lote e do plano) e `config::<T>(device)`, que descodifica o CBOR da configuração de um dispositivo para um tipo `serde`/`minicbor` do driver.
- `RawSink::push(sample)` copia a amostra para o anel SPSC do dispositivo; devolve `Err(Full)` se o anel estiver cheio, e a perda é contada (ADR 0013). Nunca bloqueia.
- `WriteAcks::set(index, status, ts_hw)`, `fill(status)`.
- `ConnectOut::device(index, info, discovered)`.
- `ActionOut::result(cbor)`; progresso e cancelamento por `cx.progress(permille, msg)` e `cx.cancelled()`.

### 3.4 `Cx` e `Link`

```rust
pub struct Cx<'a> { /* host services, borrowed for the duration of one call */ }

impl Cx<'_> {
    /// Runtime clock (trait `Clock`, ADR 0007): monotonic, UTC and clock quality.
    pub fn now(&self) -> Instant;
    /// Absolute deadline of the current call.
    pub fn deadline(&self) -> MonoNs;
    pub fn sleep_until(&self, at: MonoNs) -> impl Future<Output = ()> + MaybeSend + '_;
    pub fn report_link(&mut self, state: LinkState, detail: &str);
    /// Structured, rate-limited, never blocks (ring drained by `svc`).
    pub fn log(&self) -> &Logger;
    /// Only inside `invoke` of a long action.
    pub fn progress(&mut self, permille: u16, message: &str);
    pub fn cancelled(&self) -> bool;
    /// Device state fixed in joker.lock (identity, discovered map), for comparisons the driver
    /// needs itself (e.g. picking the SunSpec chain). The drift verdict is the core's.
    pub fn locked(&self, device: DeviceIx) -> Option<LockedDevice<'_>>;
    /// std only: runs a blocking closure on the blocking pool, bounded by the deadline.
    #[cfg(feature = "std")]
    pub fn spawn_blocking<F, R>(&self, f: F) -> impl Future<Output = Result<R, BatchFailure>> + Send
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;
}

/// The transport given by the host; the driver never opens one itself.
pub enum Link {
    None,
    Serial(SerialPort),   // embedded_io_async::{Read, Write} + read_frame(idle) + info()
    Tcp(TcpEndpoint),     // connect(deadline) -> TcpStream (embedded_io_async)
    Udp(UdpSocket),       // send, send_to (allowed networks), recv
    I2c(I2cBus),          // embedded_hal_async::i2c::I2c, restricted to the instance's addresses
    Spi(SpiDevice),       // embedded_hal_async::spi::SpiDevice
    Can(CanSocket),       // filters set by the host
    Gpio(GpioChip),       // tier N only: line requests (chardev v2 / embedded-hal)
    Paths(DevicePaths),   // tier N only: device nodes granted by the manifest (piControl, V4L2, raw socket)
}
```

Os transportes implementam os traits de `embedded-io-async` e `embedded-hal-async` 1.0, e é isso que torna o mesmo driver portável: no Linux são adaptadores sobre `tokio`, `tokio-serial`, `i2cdev`; no Joker Micro, sobre os HALs do Embassy; na conformidade, sobre duplos (§6). **Regra:** um driver tier N não abre sockets, ficheiros de dispositivo nem portas série por si; usa o `Link`. Um driver que precise de algo fora do `Link` (ioctls específicos) usa `Link::Paths` e declara-o no esquema `config` (`path`), para que o `jk doctor` verifique permissões e o `jk check` saiba que não há duplo genérico para ele.

### 3.5 Contrato de threads

| Onde | O quê | Pode | Não pode |
|---|---|---|---|
| qualquer thread, fora do nó (`jk check`, `jk build`) | `Planner::plan` | alocar; recursão limitada | I/O, relógio, aleatoriedade, estado global, `unsafe` |
| `io` (tokio multi-thread, `SCHED_OTHER` nice −5, CPUs `io`) | `open`, `connect`, `read`, `write`, `read_addr`, `write_addr`, `invoke`, `health`, `disconnect`, `Scanner::scan` | `await` de I/O; alocar (Linux); `cx.spawn_blocking` | bloquear a thread mais de 100 µs entre `await`s (sem `std::thread::sleep`, sem I/O síncrono, sem locks contendidos); criar threads; tocar nos slots ou nas imagens de processo; chamar o núcleo de ciclo |
| `rt/drv/<bus>` (`SCHED_FIFO`, CPU `rt`) | `RtPart::cycle` | ler e escrever no `RtIo` (anel de amostras, caixas latest-wins das saídas do barramento, fila de escritas temporizadas, `force_safe`); syscalls sem bloqueio do barramento (sendto em raw socket, ioctl piControl) | alocar, bloquear, `await`, logar fora do anel, ultrapassar o WCET declarado |
| Joker Micro (executor Embassy, prioridade do barramento) | os mesmos métodos de `Driver` | `await` | alocar (sem `alloc`), bloquear o executor |

- **Ninguém chama duas operações da mesma instância ao mesmo tempo** (`&mut self`); a exceção é o cancelamento de uma leitura `push` (§2.14).
- **Cancelamento:** qualquer future pode ser largado num `await` (prazo, geração nova, `push`). O driver fica consistente: por exemplo, o id de transação Modbus TCP seguinte não pode colidir com uma resposta atrasada, e o driver descarta respostas a pedidos cancelados.
- **Bloqueio detetado:** em builds de debug e na conformidade, o host mede a duração de cada `poll` do future; acima de 1 ms é `J0883` (aviso) com o nome do driver e o método. Uma biblioteca de fabricante com API bloqueante não entra no tier N: usa `spawn_blocking` com prazo, ou vai para tier X.
- **`rt-cycle`:** a parte RT recebe as saídas do barramento diretamente das caixas latest-wins (é aqui que o `force_safe` é honrado sem passar pelo grupo, 01 §3.4) e publica amostras no mesmo anel SPSC que a parte `io`. A comunicação entre a parte RT e a parte `io` do mesmo driver é por estruturas lock-free com capacidade fixa, fornecidas pelo `joker-hal` (`RtChannel<T, N>`).

### 3.6 O que o núcleo faz por cima (estágio de aquisição e de saída)

O estágio de aquisição corre no domínio `io`, na mesma tarefa do escalonador do barramento, logo a seguir a cada `read` (01 §3.3). É código do núcleo, igual para os três tiers:

1. **Validação da amostra** (§2.8): ponto no lote, uma só vez, `raw-kind` = `expect`, par de qualidade permitido. Falha → `J0869`, `bad(config_error)`.
2. **Aplicação da falha de lote** e da qualidade do link (§2.8, §2.12).
3. **Descodificação e conversão**, pela ordem de ref/03 §3 (tipo cru, `word_order`, `byte_order`, `bit`, `invert`, `invalid`, `scale_factor`, `scale`, `offset`, unidade canónica, NaN, `limits`, `enum`, `deadband`, `stuck_detect`).
4. **Tempo:** `ts_acq` (monotónico da receção), `acq_id` comum às amostras da chamada, `ts_src`/`ts_hw` do driver.
5. **Debounce** dos `bool` sem a capacidade `debounce`: nos lidos por poll, as regras D1–D5 do documento 16, §3.4 (leituras consecutivas iguais que cobrem pelo menos o `debounce`). Enquanto espera, o slot não é escrito (D3).
6. **Slot:** escrita seqlock (escritor único: este estágio, 01 §3.2), bitset `changed` por grupo, fila do historian.

O estágio de saída faz o caminho inverso: lê as caixas latest-wins e as filas ordenadas (01 §3.4), converte o valor aplicado em cru (§2.7), agrupa as escritas atómicas, gera `keepalive`, `force-safe`, `resync` e as escritas dos impulsos, chama `write` com o prazo, e regista cada `write-ack` no registo de decisões e na verificação por readback (`verify`, 01 §7).

### 3.7 `no_std` e o Joker Micro

Um driver que também serve o Joker Micro declara `micro = true` e cumpre:

- `#![no_std]` sem `alloc` na parte `Driver`; o `Planner` fica atrás da feature `plan` (só no host);
- transportes só pelos traits `embedded-io-async`/`embedded-hal-async` (`Link::Serial`, `I2c`, `Spi`, `Can`, `Gpio`);
- capacidades fixas em compilação (`heapless`, const generics), sem recursão no caminho de leitura;
- `Config` descodificada por empréstimo do CBOR em flash (`OpenArgs::config::<T<'_>>()`).

**O plano é calculado no host.** O `jk build --target mcu:…` corre o `Planner::plan` no portátil (é puro), grava o plano (CBOR determinístico) e as `point-spec` na geração compilada (embutida ou no blob, 19 §4.1; formato em ref/08), e o firmware só executa a parte `Driver`. O `jk check` soma o plano às capacidades do firmware (`J0710`). Um driver sem `micro = true` num alvo MCU é `J0875`.

**A configuração de cada dispositivo no blob.** Cada registo da secção `DEVICES` do blob (ref/08 §2.13.3) aponta para um item CBOR determinístico em `BLOBS`, com o esquema `micro-device-config` do `spec/cddl/jdp.cddl`:

| Chave | Campo | Conteúdo |
|---|---|---|
| 0 | `descriptor` | digest do descritor do driver com que o plano foi feito (§2.3) |
| 1 | `device` | o `device-spec` (§2.4): `config` (o esquema `config` do descritor, convertido como em §2.4.3), `profile-config`, `quirks`, `discovered` e `timeout-ns` |
| 2 | `points` | as `point-spec`, cada uma com o `SignalId` do sinal na geração do MCU |
| 3 | `plan` | o `read-plan` |
| 4 | `bus` | o `bus-spec`, se houver barramento |

O firmware só descodifica o que o seu driver lê (por empréstimo, sem `alloc`). **O digest do descritor viaja com a geração:** vai no item acima e no registo `REQUIRES` do driver (o campo de 8 B, com os primeiros 8 B do digest), e o `HELLO` do Joker Link anuncia os descritores dos drivers compilados no firmware (ref/07). Um plano feito com outro descritor é recusado no build (`J0773`) e, se ainda assim chegar ao MCU, no arranque (`JK_E_GEN_FIRMWARE`, `J-E2516`); é o caso D12 (§9).

Drivers do catálogo com `micro = true` na v1: `gpio`, `adc`, `pwm`, `i2c`, `spi`, `uart` (`serial-line`), `modbus-rtu` (mestre), `modbus-slave` (escravo), `can`, `onewire` (19 §3), e `esp32` (só nas placas ESP32: pads de toque, RSSI do Wi-Fi e tempo de funcionamento; ref/01 anexo A).

**Os traits de cada driver do MCU (tabela normativa).** O `embedded-hal` 1.0 não cobre todos os drivers: o ADC saiu antes da 1.0, o CAN só tem uma API *blocking*/`nb`, e o 1-Wire e o toque não têm trait. Cada driver do MCU usa só o trait desta tabela, e os módulos `chip-*` do `joker-micro` (ref/08 §2.11.2) implementam-no sobre o HAL da família. O documento 19 §3 e a ref/08 §2.11.2 remetem para aqui.

| Driver do MCU | Trait | Estado do trait | Notas |
|---|---|---|---|
| `gpio` | `embedded_hal::digital::{InputPin, OutputPin}` + `embedded_hal_async::digital::Wait` | estável (1.0) | direto |
| `i2c`, `spi` | `embedded_hal_async::{i2c::I2c, spi::SpiDevice}` | estável (1.0) | partilha do barramento com `embedded-hal-bus` |
| `uart`, `modbus-rtu`, `modbus-slave`, link série | `embedded_io_async::{Read, Write}` | **0.x**, versão exata no workspace (13 §5.1) | os tipos não aparecem na ABI C nem no blob |
| `pwm` | `embedded_hal::pwm::SetDutyCycle` | estável, só o *duty* | frequência e fase ficam no módulo `chip-*`, a partir da placa |
| `adc` | **`joker_hal::AdcRead`** (próprio) | — (não há trait do ecossistema) | implementado em cada `chip-*` |
| `can` | **`joker_hal::CanFrames`** (próprio) | — (o `embedded-can` não tem async) | implementado em cada `chip-*` sobre a API async do HAL |
| `onewire` | nenhum: um pino *open-drain* (`embedded_hal::digital` com `Flex`) e a temporização do HAL, ou uma UART | — | a temporização é do `chip-*` |
| `esp32` (toque, RSSI, tempo de funcionamento) | API do `esp-hal` (o toque está atrás de `unstable`) | instável | só no módulo `chip-esp` |

Os dois traits próprios têm a forma dos do `embedded-hal-async`, para se poderem largar quando o ecossistema normalizar um equivalente (mudança minor com adaptador):

```rust
/// joker-hal: ADC channel read (no embedded-hal 1.0 trait exists).
pub trait AdcRead {
    type Error: core::fmt::Debug;
    /// One conversion; raw code in 0..=(1 << bits) - 1. Scaling is the core's (ref/03 §3).
    async fn read(&mut self) -> Result<u16, Self::Error>;
    /// Resolution in bits, as the board declares it (`[adc] bits`, ref/03 §2.3.3).
    fn bits(&self) -> u8;
}

/// joker-hal: async CAN frames (embedded-can only has blocking/nb).
pub trait CanFrames {
    type Error: core::fmt::Debug;
    type Frame: embedded_can::Frame;
    async fn transmit(&mut self, frame: &Self::Frame) -> Result<(), Self::Error>;
    async fn receive(&mut self) -> Result<Self::Frame, Self::Error>;
}
```

### 3.8 Registo por features

Cada crate de driver exporta uma entrada:

```rust
// crates/joker-drivers/modbus/src/lib.rs
pub const MODBUS_TCP: joker_hal::DriverEntry = joker_hal::driver_entry! {
    planner: ModbusPlanner,
    driver: ModbusDriver,
    scanner: Some(ModbusScanner),
    rt: None,
};
```

`DriverEntry` guarda o descritor e ponteiros de função com tipos apagados (`plan` sobre CBOR, `open` que devolve `Box<dyn ErasedDriver>` no Linux). O `jokerd` tem um módulo de registo **gerado pelo `build.rs`** a partir das features Cargo:

| Feature Cargo | Nome em `jk runtime build --features` | Drivers |
|---|---|---|
| `drv-sim` | `sim` | `sim` (sempre presente em `dev`) |
| `drv-modbus` | `modbus` | `modbus-tcp`, `modbus-rtu`, `sunspec` |
| `drv-gpio`, `drv-i2c`, `drv-spi` | `gpio`, `i2c`, `spi` | |
| `drv-sysfs` | `sysfs` | LEDs e temperaturas de `/sys/class` (05 §10) |
| `drv-revpi` | `revpi` | |
| `drv-mqtt` | `mqtt` | |
| `drv-joker-link` | `joker-link` | driver do Joker Link (ref/07) |
| `drv-opcua`, `drv-can`, `drv-canopen`, `drv-mbus`, `drv-onewire`, `drv-serial-line` | idem | Fase 2 |
| `drv-ethercat` | `ethercat` | 1.2 |
| `drivers-wasm` | `wasm` | host tier W (§4) |
| `drivers-jdp` | `jdp` | host tier X (§5) |

- O build por omissão (`default`, o "completo" do 01 §21) inclui `sim`, `sysfs`, `modbus`, `gpio`, `i2c`, `spi`, `revpi`, `mqtt`, `joker-link`, `wasm`, `jdp`. O build `minimal` (09 ataque 12) só `sim`, `sysfs`, `modbus`, `mqtt`. Os dois têm um alvo de tamanho do binário, medido na CI (01 §21).
- `jk runtime build --features ethercat,bacnet-ip` aceita nomes de drivers e mapeia-os para as features; um nome desconhecido é erro de utilização (código de saída 1).
- O binário embute o **manifesto de drivers** (nome, versão, protocolos, capacidades, digest do descritor de cada entrada), exposto em `GET /v1/runtime/drivers` (ref/09) e reportado ao Deck no relatório (ref/10). O lock não o repete: os drivers N ficam fixados pela versão da runtime (`runtime` do lock), e num MCU pelas `features` de `[mcu.<d>]` (ref/02 §2.1.10). O `jk check --node` e o `jk apply` recusam um dispositivo cujo driver N não está no runtime do nó (`J0850`).
- No Joker Micro, o `jk build --target mcu` gera um `enum` estático com os drivers pedidos (despacho sem `dyn`), a partir das mesmas entradas.

---

## 4. Tier W: o mundo `joker:driver`

O mundo completo está em `wit/joker-driver.wit`. Esta secção dá as regras que o WIT não exprime.

**Estabilidade (regra normativa).** Na 1.0, o mundo `joker:driver` e o SDK tier W (`joker-driver-sdk`) existem como **instáveis**: o mundo leva `@unstable(feature = driver-world)`, e só os drivers **da árvore** (os do catálogo do projeto, construídos a partir de `crates/joker-drivers/` e publicados com ele) o usam em `prod`. Um componente W de fora da árvore é aceite em `dev` e recusado em `prod` (`J0860`, `error-in-prod`), como qualquer item instável. O mundo pode mudar entre minors até à **1.2**, em que passa a estável (`@since(1.2.0)`) e aberto a terceiros, com o SDK Rust e C, a suite de conformidade obrigatória e a publicação no Hub (21 §4). A regra de versão (§4.1) vale desde já; a compatibilidade de minor anterior (§4.8) só vale a partir da 1.2. Os outros documentos remetem para aqui.

### 4.1 Versão e correções face ao documento 05, §8

| No 05 §8 | Problema | Aqui |
|---|---|---|
| `package joker:driver@0.1.0` com `import joker:runtime/log@0.3.0` | viola ref/00 §1 (o `joker:driver` tem a versão do `joker:runtime` que importa) | `joker:driver@0.1.0` importa `joker:runtime/log@0.1.0` e `joker:runtime/types@0.1.0`; na 1.0, ambos `@1.0.0` |
| erros `result<_, string>` | a runtime não consegue decidir nada sobre uma string (ref/00 §8) | `io-error { kind, detail }`, `batch-failure`, `action-failure`, `write-status`: enums fechados; `error { code, message }` com J-code onde é para pessoas |
| `plan(config: list<u8>, points: list<u8>) -> result<list<u8>, string>` | bytes sem esquema | `plan-request` e `read-plan` em WIT; só o que é **específico do driver** (config, endereço, quirks, `private`) é CBOR, com esquema declarado no descritor (§2.3.1) |
| `read(batch) -> list<raw-sample>` com `ok: bool` | qualidade reduzida a um booleano | `raw-status { code, reason }` (§2.8) e `batch-failure` |
| `write(writes: list<u8>) -> list<bool>` | escritas sem tipo, acks sem causa | `committed-write` e `write-ack` com `write-status` |
| `raw-value` com `bools(list<bool>)` e sem `uint`/`text`/`none` | um ponto é um valor, não uma lista; faltavam tipos | `raw-kind` de §2.7 |
| `use raw.{raw-sample}` no mundo sem importar `raw` | WIT inválido | tipos em `interface types`, usados pelas interfaces |
| recursos de transporte sem forma de os obter | o driver não tinha como receber o `bus` | `variant link` entregue em `open` e em `scan` |
| sem ações, descoberta, saúde, prazos, estado do link, acesso por endereço | lacunas | `invoke`, `connect-info.discovered`, `scan`, `health`, `host.deadline`, `host.report-link`, `read-addr`/`write-addr` |

### 4.2 Interfaces

| Interface | Sentido | Conteúdo |
|---|---|---|
| `joker:runtime/types@0.1.0` | importada (tipos) | `quality-code`, `instant`, `error` |
| `joker:runtime/log@0.1.0` | importada | `log(level, msg, fields)`, `enabled(level)` |
| `joker:driver/types` | tipos | todos os registos do §2 |
| `joker:driver/host` | importada | `now`, `sleep-until`, `deadline`, `report-link`, `progress`, `cancelled` |
| `joker:driver/transport` | importada | `serial-port`, `tcp-endpoint`, `tcp-stream`, `udp-socket`, `i2c-bus`, `spi-device`, `can-socket`, `variant link` |
| `joker:driver/guest` | exportada | `plan`, `open`, `connect`, `read`, `write`, `read-addr`, `write-addr`, `invoke`, `scan`, `health`, `disconnect` |

Um driver que não tenha uma capacidade exporta a função na mesma (o mundo é fechado) e devolve `unsupported`/`config-error`; o SDK Rust gera essas funções a partir dos métodos por omissão do trait.

### 4.3 Secção `joker:driver-meta`

Secção custom do componente com o `descriptor` (CBOR determinístico). É obrigatória: um componente sem ela, ou com ela inválida, é `J0851`. O `jk inspect driver.wasm` mostra-a, e o Hub indexa-a. O campo `interface` tem de ser `joker:driver@<versão do pacote>` do mundo que o componente exporta; incoerente é `J0851`.

### 4.4 Modos de instanciação

| Modo | Quando | Importações disponíveis | Exportações chamadas |
|---|---|---|---|
| **plan** | `jk check`, `jk build`, `jk lock`, preparação da geração | só `log`; `host.*` e `transport.*` fazem trap com `J0882` | `plan` |
| **device** | cada instância de driver ativa (um dispositivo, ou um barramento com `instance = bus`) | todas; um `link` com o transporte da instância | `open`, depois as restantes |
| **scan** | `jk discover` | todas; `link` criado do URI | `scan` |

- **Um `Store` por instância.** O estado não é partilhado entre instâncias, e um trap afeta uma só instância.
- O componente é pré-compilado (AOT, `.cwasm`) na instalação, como as apps (01 §9), com o motor configurado como para as instâncias async: sem NaN canonicalizados obrigatórios (o driver não entra no traço determinista; as suas amostras sim).
- Em modo plan corre com *fuel* (determinista): o mesmo componente dá o mesmo plano no portátil e no nó.

### 4.5 Recursos e temporizações

| Recurso | Omissão | Máximo | Declarado em | Excedido |
|---|---|---|---|---|
| memória linear | 16 MiB | 64 MiB | `descriptor.resources.memory`; `[drivers.<n>] memory` pode baixar | `memory.grow` falha; normalmente trap → `J0868` |
| *fuel* no modo plan | 10⁹ | 10¹⁰ | `[drivers.<n>] plan_fuel` | trap → `J0855` com o detalhe |
| CPU contínua sem `await` | o guest cede a thread a cada 1 ms (epoch com *async yield*) | — | — | — |
| CPU por chamada (soma) | 50 ms | 1 s | `[drivers.<n>] cpu_per_call` | trap → `J0868` |
| prazo por chamada | §2.14 | | | cancelamento → `J0867` |
| tabelas, instâncias, recursos | 10 000 elementos de tabela; 1 024 handles de recurso | | | trap |

Um driver W corre nas threads `io`, partilhadas com os drivers N: por isso cede a thread a cada 1 ms de execução contínua, e um driver W lento atrasa os seus lotes, mas não os dos outros dispositivos.

**Falhas.** Um trap descarta a instância (`Store`), põe os pontos em `bad(not_connected)` e o link em `stopped`, e recria-a com backoff; o limiar de crashloop (`J0866`) é o da tabela do §2.12. As saídas ficam cobertas pelo fallback do dispositivo (01 §8).

### 4.6 SDKs

- **Rust:** `joker-driver-sdk` com `joker_driver_sdk::export_driver!(ModbusPlanner, ModbusDriver, scanner = ModbusScanner)`. Gera os bindings, a secção `joker:driver-meta` a partir do `DESCRIPTOR`, e adapta os transportes WIT aos traits `embedded-io-async`. Alvo `wasm32-wasip3`.
- **C:** `joker_driver.h` gerado com `wit-bindgen c`, mais `libjokerdrv` com a mesma API de `jdp.h` (§5.8) para que um driver C seja W ou X com o mesmo código: entra na 1.2 (21 §4).
- Qualquer outra linguagem que produza um componente para o mundo serve (Go com TinyGo, Zig), desde que ponha a secção `joker:driver-meta`.

### 4.7 Artefacto

Tipo de media OCI `joker.driver.v1+wasm` (ref/00 §7.1; assinado pelo papel `drivers`): o componente, com a assinatura ao lado e a anotação `org.joker.driver.interface = "joker:driver@<versão>"`. O lock fixa `digest`, `version`, `descriptor` e `wit` (ref/02 §2.1.8).

### 4.8 Compatibilidade

- A partir da 1.2 (mundo estável, §4), o host suporta o mundo `joker:driver` da sua versão e o **minor anterior**, por adaptador gerado, como no `joker:runtime` (M21). Na 1.0 e na 1.1, um driver da árvore é reconstruído com cada runtime. Um componente para um mundo mais novo do que o runtime é recusado antes do deploy (`J0859`).
- Numa minor só se acrescentam: funções novas em `guest` com comportamento por omissão definido aqui (um componente antigo não as exporta, e o adaptador devolve `unsupported`), casos novos no **fim** de enums e variantes com projeção declarada, interfaces novas, capacidades novas.
- Os casos `i2c`, `spi` e `can` de `link` fazem parte do mundo desde já. O host da 1.0 implementa `serial`, `tcp` e `udp`; um driver W que declare `buses` com `i2c`, `spi` ou `can` é recusado no `jk check` por um nó 1.0/1.1 (`J0873`) e aceite a partir da 1.2.

---

## 5. Tier X: Joker Driver Protocol (JDP)

Um driver tier X é um processo `joker-drv-<nome>` que fala JDP com o `jokerd`. Serve para SDKs binários de fabricantes, bibliotecas C (BACnet, IEC 104, GigE Vision) e código em que não se confia. O custo é IPC: +20–50 µs por lote face ao tier N (05 §1), sem cópia para streams (1.1).

### 5.1 Visão geral

```
 jokerd (utilizador joker)                                   joker-drv-<nome> (utilizador próprio)
 ┌──────────────────────────────┐   controlo: UDS SOCK_SEQPACKET, CBOR   ┌──────────────────────────┐
 │ host JDP (joker-jdp)          │◀────────────────────────────────────▶│ libjdp / joker-jdp        │
 │  supervisão, prazos, health   │   hello, plan, open, connect, …       │  worker por instância     │
 │                               │                                       │                           │
 │  memfd B (escreve o jokerd) ──┼── anel down: READ, WRITE, CANCEL ────▶│ (mapeado só leitura)      │
 │  (mapeado só leitura) ◀───────┼── anel up: SAMPLE, READ_DONE, ACK ────┼── memfd A (escreve o driver)
 │  eventfd up / eventfd down    │   notificação                          │                           │
 └──────────────────────────────┘                                       └──────────────────────────┘
```

- **Controlo** (baixo ritmo): handshake, plano, ciclo de vida, acesso por endereço, ações, scan, saúde, logs.
- **Dados** (ritmo do barramento): pedidos de leitura e escritas (`down`), amostras, fins de leitura, acks e estado do link (`up`), em dois anéis SPSC em memória partilhada.
- **Cada lado só escreve na sua memória.** O `jokerd` trata tudo o que vem do driver como hostil (§5.5.6).

### 5.2 Arranque, identidade e segurança

#### 5.2.1 Modos de arranque

| `launch` | Quem arranca o processo | Canal de controlo | Utilizador | Quando |
|---|---|---|---|---|
| `systemd` (omissão onde há systemd) | o `jokerd`, com `StartTransientUnit` por D-Bus, unidade `joker-drv-<nome>.service` (ou `joker-drv-<nome>@<dispositivo>.service` com `isolation = "per_device"`) | o driver liga-se a `/run/joker/drv/<nome>.sock` | `DynamicUser=yes`, ou `User=<user>` | produção |
| `spawn` | o `jokerd`, com `fork`+`exec` | `socketpair` herdado no fd indicado por `JDP_FD` | o do `jokerd`, ou `user` se o `jokerd` tiver `CAP_SETUID`/`CAP_SETGID` | sistemas sem systemd, `dev` |
| `external` | outra entidade (unidade systemd própria, contentor) | o driver liga-se a `/run/joker/drv/<nome>.sock` | o que essa entidade der, verificado pelo `jokerd` | integrações do fabricante |

O `jokerd` não precisa de root (01 §17). Em `systemd`, uma regra polkit instalada com o pacote autoriza o utilizador `joker` a gerir unidades `joker-drv-*` e nada mais. Em `prod`, um driver X com `launch = "spawn"` que corra com o utilizador do `jokerd` é `J0865` (`error-in-prod`): um SDK de fabricante com o mesmo utilizador do controlo anularia o isolamento.

#### 5.2.2 Sandbox (modo `systemd`)

Propriedades da unidade transitória, derivadas de `[drivers.<n>]` e dos dispositivos que o usam:

| Propriedade | Valor |
|---|---|
| `DynamicUser`, `User`, `SupplementaryGroups` | `user` (omissão: dinâmico); `groups` |
| `MemoryMax`, `CPUQuota`, `TasksMax` | `memory` (256 MiB), `cpu` (100 %), 64 |
| `OOMScoreAdjust` | +500 (01 §17) |
| `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `ProtectKernelTunables`, `ProtectKernelModules`, `LockPersonality`, `RestrictRealtime` | sim |
| `DevicePolicy=closed`, `DeviceAllow` | só os nós de `link-spec` (porta série, `/dev/i2c-*`) e `devices` |
| `RestrictAddressFamilies` | `AF_UNIX` e, se algum link for de rede, `AF_INET`, `AF_INET6`; `AF_CAN` para `can`; `AF_PACKET` só com `raw_net = true` |
| `IPAddressDeny=any`, `IPAddressAllow` | os endereços dos dispositivos e as redes de `net` e de `udp` |
| `StateDirectory` | `joker-drv/<nome>` (o único sítio onde pode escrever, para caches do SDK) |
| `SetCredential` | `jdp-token` (§5.2.3) |
| `Environment` | `JDP_SOCKET`, `JDP_TOKEN_FILE=$CREDENTIALS_DIRECTORY/jdp-token`, `JDP_DRIVER`, `JDP_NODE`, e `env` |
| `Restart` | `no`: quem reinicia é o `jokerd` (§5.7) |

#### 5.2.3 Autenticação

Três verificações, todas obrigatórias fora de `spawn`:

1. **Permissões do socket.** `/run/joker/drv/` é `0710 joker:joker-drv`; cada `<nome>.sock` é `0660 joker:joker-drv`, criado pelo `jokerd` antes de arrancar o driver e apagado depois. As unidades de drivers têm o grupo `joker-drv` como suplementar; nenhum outro utilizador tem acesso.
2. **Credenciais do par.** `SO_PEERCRED` na ligação: o `pid` tem de pertencer ao cgroup da unidade que o `jokerd` arrancou (lido de `/proc/<pid>/cgroup`), ou, em `external`, o `uid` tem de ser o de `user`. Falha: `J0862`, ligação fechada.
3. **Token.** 32 bytes aleatórios por arranque, entregues por credencial systemd (ou, em `spawn`, num ficheiro `0400` num diretório temporário, apagado quando o `hello` chega). O `hello` leva-o; diferente, ou reutilizado, é `J0862`.

Em `spawn` a ligação é um `socketpair` e não há socket no sistema de ficheiros; o token continua obrigatório.

#### 5.2.4 Chaves de `[drivers.<nome>]`

A lista normativa das chaves, com tipos, omissões e validação, está na **ref/01 §5.7** (`tier`, `source`, `digest`, `exec`, `args`, `launch`, `user`, `groups`, `memory`, `cpu`, `cpu_per_call`, `plan_fuel`, `isolation`, `net`, `devices`, `raw_net`, `env`, `restart`). Aqui fica o que cada uma faz no host:

```toml
[drivers.bacnet-ip]
tier      = "x"
exec      = "/usr/lib/joker/drivers/joker-drv-bacnet-ip"
launch    = "systemd"
memory    = "256 MiB"
cpu       = "50 %"
isolation = "process"
net       = ["10.20.0.0/16"]
env       = { BACNET_LOG = "warn" }

[drivers.dlms-cosem]
tier   = "w"
source = "oci://registry.example.com/joker/drivers/dlms-cosem:0.3.1"
memory = "8 MiB"
```

| Chave | Tier | Efeito |
|---|---|---|
| `launch` | x | modo de arranque (§5.2.1); `spawn` com o utilizador do `jokerd` em `prod` é `J0865` |
| `user` | x | `"dynamic"` (omissão com `launch = "systemd"`): `DynamicUser=yes`; com `launch = "spawn"`, a omissão é `joker-drv-<driver>`; em `external`, o `uid` que o `SO_PEERCRED` tem de mostrar (§5.2.3) |
| `groups` | x | `SupplementaryGroups` (§5.2.2) |
| `memory` | w, x | W: limite da memória linear (§4.5); X: `MemoryMax` |
| `cpu` | x | `CPUQuota` |
| `cpu_per_call`, `plan_fuel` | w | §4.5 |
| `isolation` | x | `per_device`: uma unidade `joker-drv-<nome>@<dispositivo>` por dispositivo |
| `net`, `devices`, `raw_net` | x | `IPAddressAllow`, `DeviceAllow`, `AF_PACKET` (§5.2.2) |
| `env` | x | `Environment`, além das variáveis `JDP_*` |
| `restart` | w, x | backoff mínimo e máximo do reinício (§4.5, §5.7) |

### 5.3 Canal de controlo

#### 5.3.1 Transporte e enquadramento

- Socket Unix `SOCK_SEQPACKET`: cada datagrama é **uma** mensagem CBOR completa, ≤ 64 KiB. Não há prefixo de comprimento.
- Os descritores de ficheiros (memfd, eventfd, streams) vão em `SCM_RIGHTS` no datagrama da mensagem que os declara (`rings`, `stream-open`), e só nessas; descritores em qualquer outra mensagem são fechados e é `J0863`.
- O JDP é **só Linux** na v1 (memfd, eventfd, seqpacket). Em macOS e Windows de desenvolvimento, os drivers X não existem; usa-se `sim` ou o próprio driver compilado como N.

#### 5.3.2 Mensagens

Cada mensagem é um mapa CBOR com chaves inteiras (`spec/cddl/jdp.cddl`): `0` tipo, `1` id do pedido, `2` instância, `3` estado (só respostas), `4` J-code, `5` texto; o corpo usa chaves ≥ 16. As estruturas partilhadas (`plan-request`, `read-plan`, `connect-info`…) são o espelho dos registos WIT: a chave de cada campo é a sua posição no registo, a partir de 0; `option` vazio é chave ausente; `variant` é `[caso, payload]`; `enum` é o índice do caso.

| Tipo | Nome | Sentido | Resposta | Corpo |
|---|---|---|---|---|
| 1 | `hello` | D → J | 2 | versões JDP `[min, max]`, `descriptor`, token, pid, SDK |
| 2 | `welcome` | J → D | — | versão escolhida, nó, geração, `limits` |
| 3 | `plan` | J → D | 4 | `plan-request` |
| 4 | `plan-res` | D → J | — | `read-plan` ou lista de `diagnostic` |
| 5 | `rings` | J → D | 6 | tamanho da entrada, capacidades, tamanhos das memfd; 4 fds |
| 6 | `rings-res` | D → J | — | — |
| 7 | `open` | J → D | 8 | instância, `plan-request`, `read-plan`, `link-spec` |
| 8 | `open-res` | D → J | — | — |
| 9 | `connect` | J → D | 10 | prazo |
| 10 | `connect-res` | D → J | — | lista de `connect-info` |
| 11 | `disconnect` | J → D | 12 | prazo |
| 12 | `disconnect-res` | D → J | — | — |
| 13 | `close` | J → D | 14 | — |
| 14 | `close-res` | D → J | — | — |
| 15 | `read-addr` | J → D | 16 | `addr-read`, prazo |
| 16 | `read-addr-res` | D → J | — | `raw-value` ou `batch-failure` |
| 17 | `write-addr` | J → D | 18 | `addr-write`, prazo |
| 18 | `write-addr-res` | D → J | — | `batch-failure` opcional |
| 19 | `invoke` | J → D | 20 | `action-call`, prazo |
| 20 | `invoke-res` | D → J | — | resultado (CBOR) ou `action-failure` |
| 21 | `progress` | D → J | notificação | chamada, permilagem, texto |
| 22 | `cancel` | J → D | notificação | chamada |
| 23 | `scan` | J → D | 25 | `scan-request`, `link-spec`, prazo |
| 24 | `scan-hit` | D → J | notificação | id do scan, `scan-hit` |
| 25 | `scan-res` | D → J | — | — |
| 26 | `health` | J → D | 27 | — |
| 27 | `health-res` | D → J | — | `health-report` por instância |
| 28 | `log` | D → J | notificação | nível, texto ≤ 1 KiB, ≤ 16 campos |
| 29 | `bye` | ambos | notificação | motivo (`shutdown`, `generation`, `restart`, `protocol`, `fatal`) |
| 30 | `error` | ambos | notificação | J-code, texto, id ofensor |
| 31–39 | — | — | — | reservados 1.x |
| 40–42 | `stream-open`, `stream-res`, `stream-close` | D ↔ J | | **1.1** (§5.6) |

**Estado** (`status`): `ok` 0, `error` 1 (ver `code`), `unsupported` 2, `invalid` 3, `timeout` 4, `busy` 5, `denied` 6, `version` 7.

**Regras:**

- Ids escolhidos por quem pede, únicos entre os seus pedidos pendentes. Uma resposta a um id desconhecido gera `error` e é ignorada.
- Tolerância (ref/00 §1): chaves desconhecidas ignoradas; um pedido de tipo desconhecido tem resposta `unsupported` com o mesmo id; uma notificação de tipo desconhecido é ignorada.
- Por instância, no máximo **um** pedido de ciclo de vida (`open`, `connect`, `disconnect`, `close`) pendente; `read-addr`, `write-addr` e `invoke` são serializados pelo `jokerd` com as escritas da mesma instância (§2.14). Um segundo pedido simultâneo da mesma classe recebe `busy` (é um bug do `jokerd`, e o driver só o reporta).
- Os prazos no corpo são instantes `CLOCK_MONOTONIC` do nó, em ns.

#### 5.3.3 Sequência

```
driver                                            jokerd
  │ connect(/run/joker/drv/x.sock) ou fd herdado    │
  │── hello{jdp:[1,1], descriptor, token, pid} ───▶│  verifica par, token, descritor, lock
  │◀───────── welcome{st:ok, jdp:1, limits} ────────│
  │◀───────── rings{…} + SCM_RIGHTS(A,B,up,down) ───│
  │── rings-res{st:ok} ───────────────────────────▶│
  │◀───────── plan{plan-request} ───────────────────│  (só se o plano não estiver em cache para
  │── plan-res{read-plan} ────────────────────────▶│   este digest de descritor e este pedido)
  │◀───────── open{inst:0, request, plan, link} ────│
  │── open-res{st:ok} ────────────────────────────▶│
  │◀───────── connect{inst:0, deadline} ────────────│
  │── connect-res{connect-info…} ─────────────────▶│  deriva (J0401), passos [connect], fallback
  │◀── anel down: READ(on-demand), WRITE(resync) ───│
  │── anel up: SAMPLE…, READ_DONE, WRITE_ACK ─────▶│  link up; poll periódico; push armado
  │              …                                   │
  │◀───────── health{} a cada 5 s ──────────────────│
  │── health-res{…} ──────────────────────────────▶│
  │◀───────── bye{generation} ──────────────────────│  geração nova: disconnect, close
```

### 5.4 Handshake e versões

- O driver envia `hello` como **primeira** mensagem, em ≤ 5 s depois de arrancar; outra mensagem primeiro, ou nada, é `J0863` e o processo é terminado.
- O `jokerd` escolhe a maior versão JDP comum. Sem versão comum: `welcome{st: version}`, `J0861`, e o driver termina com código 3. Na v1 só existe `jdp = 1`.
- O `jokerd` verifica, pela ordem: par e token (`J0862`); descritor bem formado (`J0851`); `name` e `tier` iguais aos de `[drivers]`; `version` e digest do descritor iguais aos do lock (`J0885`); digest do executável igual ao do lock (verificado antes de arrancar; ref/02 §2.1.8).
- **Compatibilidade:** uma versão nova do JDP só acrescenta tipos de mensagem, chaves opcionais e tipos de entrada nos anéis. O `jokerd` de uma minor suporta o `jdp` corrente e o anterior; um driver compilado com `libjdp` mais recente fala a versão que o `welcome` escolher (a `libjdp` implementa as anteriores).

### 5.5 Plano de dados

#### 5.5.1 Memórias partilhadas

O `jokerd` cria duas memfd (`MFD_CLOEXEC | MFD_ALLOW_SEALING`), dimensiona-as, sela-as com `F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL` e passa-as no `rings`:

- **A** (escrita pelo driver): anel `up` (entradas e `head`) e `tail` do anel `down`;
- **B** (escrita pelo `jokerd`): anel `down` (entradas e `head`) e `tail` do anel `up`. Antes de a passar, o `jokerd` mapeia-a com escrita e aplica `F_SEAL_FUTURE_WRITE`: o driver só a consegue mapear só de leitura.

Assim, cada índice e cada entrada tem **um só escritor** (o princípio do slot, 01 §3.2, aplicado entre processos), e um driver não consegue corromper os pedidos do `jokerd` nem os índices que o `jokerd` usa.

| Memfd A (driver escreve) | Deslocamento | Tamanho |
|---|---|---|
| cabeçalho (escrito pelo `jokerd` antes de passar; o `jokerd` nunca o relê) | 0 | 64 |
| `up.head` (u64) | 64 | 64 (linha própria) |
| `down.tail` (u64) e `down.sleeping` (u32 em +8) | 128 | 64 |
| contadores do driver: `up.dropped` (u64), `up.published` (u64) | 192 | 64 |
| entradas do anel `up` | 256 | `up_cap` × 64 |

| Memfd B (`jokerd` escreve) | Deslocamento | Tamanho |
|---|---|---|
| cabeçalho | 0 | 64 |
| `down.head` (u64) | 64 | 64 |
| `up.tail` (u64) e `up.sleeping` (u32 em +8) | 128 | 64 |
| contadores do `jokerd`: `down.dropped`, `down.published` | 192 | 64 |
| entradas do anel `down` | 256 | `down_cap` × 64 |

**Cabeçalho** (64 B, little-endian): `magic` `"JDPRINGA"` ou `"JDPRINGB"` (8 B), `version` u16 = 1, `entry_size` u16 = 64, `up_cap` u32, `down_cap` u32, `flags` u32 = 0, reservado (40 B, zeros).

**Capacidades:** potências de 2. Omissão: `up` 4 096 entradas (256 KiB), `down` 1 024 (64 KiB). Máximo: 65 536 cada. O `jokerd` dimensiona o `up` para caber pelo menos 4 × o maior lote `poll` e 256 entradas `push` por instância.

#### 5.5.2 Índices e ordem de memória

- `head` e `tail` são u64 monotónicos (nunca dão a volta na prática); a posição da entrada é `índice & (cap − 1)`.
- **Produtor:** lê `tail` com *acquire*; se `head − tail == cap`, o anel está cheio (a amostra é perdida e `dropped` incrementado; um `WRITE` nunca é perdido: o `jokerd` espera por espaço, porque as escritas são limitadas pelas caixas latest-wins); escreve a(s) entrada(s); publica `head` com *release*.
- **Consumidor:** lê `head` com *acquire*; copia as entradas `[tail, head)` para memória privada; publica `tail` com *release*.
- Os atómicos são `AtomicU64` sem lock (em armv7, `ldrexd`/`strexd`); o `jokerd` recusa um alvo sem atómicos de 64 bits sem lock (não há nenhum nas plataformas de 01 §22).

#### 5.5.3 Notificação por `eventfd`

Dois `eventfd` (`EFD_NONBLOCK | EFD_CLOEXEC`, modo contador): `up` acorda o `jokerd`, `down` acorda o driver.

- **Consumidor**, antes de adormecer: `sleeping = 1` (*SeqCst*); relê `head`; se houver entradas, `sleeping = 0` e consome; senão, espera no `eventfd` (poll/epoll, ou o reator tokio), e ao acordar lê o contador e faz `sleeping = 0`.
- **Produtor**, depois de publicar `head`: *fence SeqCst*; se `sleeping == 1` do consumidor, escreve 1 no `eventfd`.
- O `sleeping` de cada anel está na memória do **consumidor** (A para o `down`, B para o `up`).

Sem a cláusula do `sleeping`, cada lote custava duas syscalls; com ela, um consumidor ocupado não paga nenhuma.

#### 5.5.4 Entradas

Todas as entradas têm 64 bytes, little-endian, com um cabeçalho comum de 16 bytes:

| Desl. | Tipo | Campo |
|---|---|---|
| 0 | u8 | `kind` |
| 1 | u8 | `flags`: bit 0 `more` (seguem entradas `CONT`), bit 1 `end` (última da chamada), bit 2 `has_ts_src`, bit 3 `has_ts_hw`, bit 4 `has_at`, bits 5–7 a 0 |
| 2 | u16 | `inst` |
| 4 | u32 | `a`: ponto, lote ou 0 |
| 8 | u64 | `tag`: id da leitura ou `seq` da escrita |

**Anel `up` (driver → `jokerd`):**

| `kind` | Nome | `a` | `tag` | Corpo (16–63) |
|---|---|---|---|---|
| 1 | `SAMPLE` | ponto | id da `READ` (ou do armar do lote `push`) | 16 u8 `q_code`; 17 u8 `q_reason`; 18 u8 `raw_kind`; 19 u8 0; 20 u16 `raw_len` (bytes do payload; 8 nos escalares); 22 u16 0; 24 i64 `ts_src`; 32 u64 `ts_hw`; 40–63 payload (24 B) |
| 2 | `CONT` | índice da continuação (1..) | o da entrada anterior | 16–63: 48 B de payload |
| 3 | `READ_DONE` | lote | id da `READ` | 16 u8 resultado (`0xFF` ok, ou o caso de `batch-failure`); 17 u8 0; 18 u16 amostras enviadas; 20 u32 0; 24–63 detalhe UTF-8 (≤ 40 B, zeros a completar) |
| 4 | `WRITE_ACK` | ponto | `seq` | 16 u8 `write-status`; 17–23 0; 24 u64 `ts_hw` (com `has_ts_hw`); 32 u64 id da chamada `WRITE`; 40–63 detalhe (≤ 24 B) |
| 5 | `LINK` | 0 | 0 | 16 u8 `link-state`; 17–23 0; 24–63 detalhe (≤ 40 B) |

**Anel `down` (`jokerd` → driver):**

| `kind` | Nome | `a` | `tag` | Corpo |
|---|---|---|---|---|
| 16 | `READ` | lote | id da leitura (≠ 0) | 16 u64 prazo; 24 u8 `batch-kind`; 25–63 0 |
| 17 | `WRITE` | ponto | `seq` | 16 u8 `write-kind`; 17 u8 `raw_kind`; 18 u16 `raw_len`; 20 u32 grupo atómico (0 = nenhum, senão id + 1); 24 u16 índice; 26 u16 total; 28 u32 id da chamada; 32 u64 `at` (com `has_at`); 40 u64 prazo; 48–63 payload (16 B) |
| 18 | `CANCEL` | 0 | id da leitura a cancelar | 16–63 0 |
| 19 | `CONT` | índice | o da entrada anterior | 16–63: 48 B de payload |

**Payload:** `boolean` u64 0/1; `int` i64; `uint` u64; `float` bits de f64; `regs` palavras u16 LE pela ordem do protocolo; `bytes` tal qual; `text` UTF-8. Um valor maior do que o espaço da primeira entrada continua em entradas `CONT` consecutivas (flag `more` em todas menos a última); `raw_len` ≤ 256, logo no máximo 5 continuações no `up` e 6 no `down`. Uma sequência de continuações é escrita e publicada de uma vez (um só `head`).

**Chamadas:** uma chamada `write` do núcleo são as entradas `WRITE` com o mesmo id de chamada, com `end` na última; o driver só começa a aplicá-las quando vê a `end` (e assim um grupo atómico chega inteiro). Uma leitura é uma `READ`; as respostas são as `SAMPLE` com o `tag` dela e uma `READ_DONE`.

**Lotes `push`:** a `READ` de um lote `push` **arma-o**. Depois disso o driver emite `SAMPLE` com o `tag` do armar sempre que houver dados, sem `READ_DONE`, até um `CANCEL` com esse `tag` ou um `disconnect`. No tier X não há cancelamento para escritas: o processo é concorrente por natureza, e o SDK entrega os dados de `push` por `jdp_push()` a partir de qualquer thread.

#### 5.5.5 Ritmo e latência

- O `jokerd` publica uma `READ` e espera a `READ_DONE` até ao prazo; o estágio de aquisição corre sobre as `SAMPLE` recebidas quando chega a `READ_DONE` (todas com o mesmo `acq_id`) ou, num lote `push`, a cada drenagem.
- O custo alvo de ida e volta de um lote de 20 pontos, com o consumidor acordado, é ≤ 20 µs num CM4, e ≤ 50 µs com as duas notificações por `eventfd` (a medir na Fase 1; 05 §1).

#### 5.5.6 Validação pelo `jokerd`

O `jokerd` trata a memfd A e todas as mensagens do driver como entrada hostil:

1. `up.head` é lido uma vez por drenagem; `head < tail` ou `head − tail > up_cap` é `J0863`.
2. Cada entrada é **copiada** para memória privada antes de ser interpretada (o driver pode alterá-la entretanto).
3. `kind` conhecido; `inst` aberta; `a` dentro dos pontos ou lotes da instância; `tag` de uma leitura pendente ou de um `push` armado; `raw_len` ≤ 256 e coerente com `raw_kind`; continuações completas e na ordem; `text` UTF-8 válido; bits de `flags` reservados a zero.
4. Uma violação em 1 ou num `kind` desconhecido termina o driver (`J0863`: SIGKILL, os pontos a `bad(not_connected)`, reinício com backoff). Uma violação em 3 dentro de uma entrada bem formada descarta a entrada com `J0869` e conta-a; 100 por minuto é tratado como 1.
5. As mensagens de controlo são validadas contra o CDDL; os `log` e `progress` acima do ritmo de `limits` são descartados e contados (`J0886`).

### 5.6 Streams (reservado para 1.1)

Os streams (M4) de drivers X (câmaras GigE Vision, SDKs de câmaras) usam descritores passados por `SCM_RIGHTS`, sem cópia:

- `stream-open` (D → J), com o ponto do stream, o número e o tamanho dos buffers, o formato e as dimensões, e os fds: os n buffers (memfd ou dmabuf), uma memfd com dois anéis de descritores de frame e dois `eventfd`;
- anel `frames` (driver → `jokerd`): entradas de 64 B com índice do buffer, `seq`, `ts_hw`, bytes úteis, `stride`, flags;
- anel `release` (`jokerd` → driver): índice do buffer devolvido quando a última referência ao frame é largada (01 §11);
- semântica `latest` ou `queue(n)` do manifesto, aplicada pelo `jokerd` (um driver lento em devolver buffers perde frames, contados).

Na 1.0 estas mensagens recebem `unsupported`, e um perfil com `kind = "stream"` é `J0807`. O layout exato dos anéis de frames fixa-se na 1.1, no mesmo formato de entradas de §5.5.4.

### 5.7 Supervisão e reinício

| Estado do processo | Entra | Sai |
|---|---|---|
| `starting` | o `jokerd` arranca a unidade ou faz `spawn` | `hello` válido → `ready`; 5 s sem `hello` → `dead` |
| `ready` | handshake, `rings`, `open` e `connect` das instâncias | geração nova → `draining`; morte → `dead` |
| `draining` | `bye{generation}` ou `bye{shutdown}` | `bye` do driver ou 2 s → SIGTERM; mais 3 s → SIGKILL |
| `dead` | EOF no socket, `pidfd`/`SIGCHLD` (`spawn`), unidade `failed` (`systemd`), 3 `health` sem resposta (15 s), `J0863`, prazo de cancelamento excedido | → `backoff` |
| `backoff` | | depois de 1 s × 2ⁿ (máx. 5 min, jitter ±20%) → `starting` |
| `crashloop` | o limiar do tier X na tabela do §2.12 (`J0866`, alarme crítico `joker.driver.<n>.crashloop`) | 10 min seguidos em `ready` → contador a zero |

- **Na morte:** todos os pontos de todas as instâncias do processo passam de imediato a `bad(not_connected)` e o link a `stopped`; as escritas pendentes recebem `not-connected` no registo de decisões; as caixas latest-wins guardam o último valor aplicado, que é reenviado como `resync` depois do próximo `connect` (§2.1); entretanto, o fallback do dispositivo (M1) cobre as saídas, como com um driver N em baixo (01 §20).
- **Encravamento:** um prazo de chamada excedido gera `CANCEL` (leitura) e, se a resposta não chegar em mais 500 ms, conta como um `health` falhado; três seguidos matam o processo (`J0881`).
- **Isolamento `process` vs `per_device`:** com `process`, um crash afeta todos os dispositivos desse driver; com `per_device`, só um, ao custo de um processo por dispositivo.
- **Geração nova:** se nada mudou para o driver (mesmo digest, mesmos pedidos), o processo e as instâncias continuam; se mudou só o pedido de uma instância, o `jokerd` faz `disconnect`/`close`/`open`/`connect` dessa instância; se mudou o executável, `bye{generation}` e processo novo.
- **Logs:** as mensagens `log` vão para o log estruturado do `jokerd` com `driver=<n> inst=<i>`; o stdout e o stderr do processo vão para o journal da unidade (ou são descartados em `spawn`, depois de 64 KiB por minuto).

### 5.8 SDK C: `libjdp`

O header normativo é `spec/c/jdp.h`. O modelo:

| Aspeto | Regra |
|---|---|
| entrada | `int jdp_main(argc, argv, &driver, user)`: lê o ambiente, faz o handshake e serve até ao `bye`; devolve o código de saída (0 `bye` limpo, 2 ambiente, 3 versão ou autenticação, 4 protocolo) |
| callbacks | `jdp_driver_t`: `plan`, `open`, `connect`, `read`, `write`, `read_addr`, `write_addr`, `invoke`, `scan`, `health`, `disconnect`, `close`; os opcionais a `NULL` respondem `unsupported` |
| threads | uma thread de controlo, uma de dados, e **uma thread de trabalho por instância**; os callbacks de uma instância são chamados um de cada vez, nessa thread, e **podem bloquear** até ao prazo que recebem (é o caso de uso dos SDKs de fabricante bloqueantes) |
| thread-safe | `jdp_push`, `jdp_report_link`, `jdp_log`, `jdp_progress`, `jdp_cancelled` |
| memória | os ponteiros passados a um callback só valem durante o callback; a `libjdp` copia tudo o que o driver lhe dá |
| `push` | os lotes `push` não chamam `read`: o driver chama `jdp_push(inst, batch, samples, n)` quando tem dados; antes de o lote estar armado, ou depois de cancelado, devolve `JDP_E_CLOSED` |
| CBOR | leitor mínimo por caminho (`jdp_cbor_get_uint(config, "unit_id", &u)`) para `config`, `addr`, `quirks` e argumentos |
| ABI | `JDP_ABI_VERSION_MAJOR`/`MINOR`; estruturas com `size` no primeiro campo crescem no fim |

Esqueleto de um driver (ilustrativo):

```c
#include "jdp.h"

static jdp_status_t plan(void *u, const jdp_plan_request_t *rq, jdp_plan_builder_t *b) {
    (void)u;
    for (size_t i = 0; i < rq->n_points; i++) {           /* um lote por ponto: o mais simples */
        const jdp_point_t *p = &rq->points[i];
        if (p->direction == JDP_DIR_OUTPUT) continue;
        jdp_plan_add_batch(b, p->device, JDP_BATCH_POLL, &p->id, 1, p->period_ns, 2000000, NULL, 0);
    }
    return JDP_OK;
}
/* … open, connect, read, disconnect … */
int main(int argc, char **argv) {
    static const uint8_t meta[] = { /* descriptor em CBOR, gerado por jk driver meta */ 0xA0 };
    jdp_driver_t d = { .size = sizeof d, .abi_major = JDP_ABI_VERSION_MAJOR,
                       .abi_minor = JDP_ABI_VERSION_MINOR,
                       .descriptor = { meta, sizeof meta }, .plan = plan /* , … */ };
    return jdp_main(argc, argv, &d, NULL);
}
```

`jk driver meta <driver>.driver.toml` gera o descritor CBOR a partir de um ficheiro TOML com os mesmos campos, pelos nomes de §2.3, na raiz (para quem não escreve Rust). O sufixo `.driver.toml` segue a ADR 0026; o ficheiro reconhece-se pelas chaves `name`, `tier` e `protocols` na raiz, e outro conteúdo é `J0803` (ref/03 §2.1.6). O esqueleto acima não é um driver válido (faltam `open`, `connect`, `read`, `disconnect` e um descritor não vazio); serve para mostrar a forma.

### 5.9 Artefacto tier X

Tipo de media OCI `joker.driver.v1+tar` (ref/00 §7.1; assinado pelo papel `drivers`): um índice multi-plataforma (`linux/arm64`, `linux/amd64`, `linux/arm/v7`), e em cada plataforma uma camada tar com `bin/joker-drv-<nome>`, `lib/` (bibliotecas do fabricante, carregadas com `RPATH=$ORIGIN/../lib`) e `descriptor.cbor`. O `jokerd` extrai-o para `/var/lib/joker/drivers/<nome>/<digest>/` (só leitura para o utilizador do driver). O lock fixa o digest do executável da arquitetura do alvo (ref/02 §2.1.8); um executável que não é ELF da arquitetura do nó é `J0872`.

### 5.10 SDK Rust: `joker-jdp`

```rust
fn main() -> std::process::ExitCode {
    // The same Planner + Driver as tier N; transports are real sockets/ports inside the sandbox.
    joker_jdp::serve::<ModbusPlanner, ModbusDriver>(joker_jdp::Options {
        scanner: Some(joker_jdp::scanner::<ModbusScanner>()),
        ..Default::default()
    })
}
```

- `serve` corre um runtime tokio `current_thread` por instância (o equivalente à thread de trabalho da `libjdp`) e implementa o `Link` com os recursos do `link-spec` (abre a porta série, liga ao `host:porta`), dentro das restrições da sandbox.
- Os lotes `push` usam o mesmo `read` cancel-safe do tier N: o `serve` mantém a leitura armada e envia as amostras à medida que chegam.
- `cx.spawn_blocking` usa threads do processo do driver (não há limite de bloqueio no tier X, só o prazo).

---

## 6. Suite de conformidade de drivers

Uma suite, **a mesma para os três tiers**, que diz se um driver cumpre este contrato. Pedida para a 1.2 (21 §4: SDK tier W e publicação no Hub); a interface fica fixada na v1, e os drivers do catálogo da 1.0 já passam por ela.

### 6.1 Como se corre

| Forma | Para quê |
|---|---|
| `jk driver test <ref> [--fixture <f>] [--case <glob>] [--json] [--report <out.cbor>]` | `<ref>` é `modbus-tcp` (N, do `jokerd` local), `w:./target/dlms.wasm` ou `x:./build/joker-drv-acme` |
| `joker_drvtest::run::<P, D>(fixture)` em `cargo test` | driver N ou o mesmo código antes de ser W ou X |
| CI do catálogo e verificação de publicação do Hub | exige o relatório assinado (`J0880`) |

O executor (`joker-drvtest`) é um host completo com o **mesmo** estágio de aquisição, o mesmo escalonador e os mesmos hosts W e X do `jokerd`, e um relógio virtual para os tiers N e W (o X corre em tempo real, com prazos escalados × 4).

### 6.2 Fixture

A suite usa o **esquema único de conformidade** da ref/03 §2.6 (D10: `[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`), com `kind = "driver"`. O que difere do núcleo e das classes é a *fixture*: cada driver traz um ficheiro que diz como o testar, com a configuração de um dispositivo, um perfil de teste e o **duplo** do equipamento (o que está do outro lado do transporte). Chama-se `<driver>.fixture.toml` (ADR 0026: o nome do driver da chave `[fixture] driver`, um ficheiro por driver, porque o mesmo pacote pode ter `modbus-tcp` e `modbus-rtu`), começa pela tabela `[fixture]` (sem ela, ou noutro sufixo, é `J0803`, ref/03 §2.1.6) e não é uma suite: é passado com `--fixture`, e os casos genéricos de `conformance/drivers/*.conformance.toml` correm contra ele.

```toml
# drivers/modbus/conformance/modbus-tcp.fixture.toml
schema = 1

[fixture]
driver  = "modbus-tcp"
profile = "conformance/modbus-test@1"      # perfil do próprio fixture (ref/03)
config  = { address = "127.0.0.1:0", unit_id = 1 }
poll    = "100ms"
timeout = "200ms"

[double]
kind = "modbus-server"                     # duplo embutido na suite
registers = { "hr.40100" = 0, "ir.30000" = [0x0001, 0x86A0], "ir.30010" = 2 }
exceptions = { "hr.40999" = 2 }            # endereço ilegal → exceção 2

[values]                                   # valores crus que os casos esperam ler, por ponto do perfil
"p_ac"  = { raw = [1, 34464] }             # regs, pela ordem do protocolo
"state" = { raw = [2] }
```

| Tabela | Chaves | Significado |
|---|---|---|
| `[fixture]` | `driver` (a forma do §2.2), `profile` (ref/03), `config` (validada pelo esquema `config` do descritor, §2.3.1), `bus` (tabela com as chaves de `[buses.*]` da ref/01, se o driver precisar de um), `poll`, `timeout` | o dispositivo sob teste |
| `[double]` | `kind` e as chaves do duplo | o equipamento simulado |
| `[values]` | `"<ponto do perfil>" = { raw }` | o valor cru que o duplo serve, na forma de `raw-kind` do §2.7; é o que `equals` compara quando um caso usa `from_fixture` |

Duplos embutidos: `modbus-server` (TCP e RTU), `serial-script`, `tcp-script` e `udp-script` (transcrição: bytes esperados → bytes de resposta, silêncios, atrasos, fecho), `can-loop`, `i2c-regs` (mapa de registos por endereço). Um driver cujo protocolo não tenha duplo embutido usa uma transcrição, ou um duplo externo: `double = { kind = "external", exec = "…", ready = "tcp:127.0.0.1:4840" }`.

### 6.3 Casos

Os casos genéricos vivem em `conformance/drivers/*.conformance.toml`, um ficheiro por suite, escritos à mão a partir desta referência (a regra do Cravo, 16 §8), e são parametrizados pela fixture.

```toml
# conformance/drivers/read.conformance.toml
schema = 1

[suite]
kind  = "driver"
id    = "drv.read"
spec  = ["ref/06 §2.8", "ref/06 §2.12", "ref/06 §2.14"]
tiers = ["n", "w", "x"]

[[case]]
id    = "timeout"
level = "basic"
do = [
  { at = "0s",    call = "connect", key = "c" },
  { at = "100ms", fault = "silence", for = "1s" },
  { at = "100ms", call = "read", batch = "first", key = "r1" },
  { at = "400ms", call = "read", batch = "all" },
  { at = "700ms", call = "read", batch = "all" },
  { at = "1s",    call = "read", batch = "all", key = "r4" },
]
expect = [
  { within = "0s .. 100ms",                     call = "c",  ok = true },
  { within = "100ms .. 100ms + timeout + 50ms", call = "r1", failure = "timeout", link = "up" },
  { within = "1s .. 1s + timeout + 50ms",       call = "r4", failure = "timeout", link = "down" },
]
```

**`[suite]` e `[[case]]`:** as chaves da ref/03 §2.6.1, com `kind = "driver"`, e mais duas: `tiers` em `[suite]` (lista de `n`, `w`, `x`: onde a suite corre; omissão: os três) e `level` em `[[case]]` (`basic` \| `full`, §6.4). `requires` são **capacidades do driver** (§2.13), mais `micro` (o campo 14 do descritor) e `systemd` (o executor tem `launch = "systemd"`): um caso cujo requisito o driver não cumpre fica `skip`. Um caso só de um tier (a coluna "Requer" do §6.4) vive numa suite com esse `tiers`.

**Linha temporal.** Os instantes são os da ref/03 §2.1.3, contados a partir do `open` da instância, em tempo virtual (N e W) ou real (X, com os prazos escalados × 4). **Exceção declarada ao D10** (ref/03 §5): nos instantes e intervalos de uma suite `driver`, e só aí, o termo `timeout` é o `timeout` da fixture (já escalado no X), para que um caso sirva a qualquer driver sem reescrever os números (`"100ms + timeout + 50ms"`).

**Ações** em `do` (cada uma com `at`; `key` opcional, para as expectativas a referirem):

| Ação | Forma | Semântica |
|---|---|---|
| chamada | `{ at, call = "<operação>", key, batch \| writes \| addr \| action \| args }` | uma operação do §2.1: `plan`, `plan_twice` (dois planos do mesmo pedido, em processos diferentes), `open`, `connect`, `read` (`batch = "first"`, `"all"` ou o índice), `write` (`writes` = lista de `{ point, value, kind }`), `read_addr`, `write_addr` (`addr`, `value`), `invoke` (`action`, `args`), `scan`, `health`, `disconnect`, `cancel` (larga a chamada em curso) |
| falha | `{ at, fault = "<tipo>", for }` | no duplo: `silence`, `delay`, `close`, `garbage`, `exception`, `reset`; e `kill` (X: mata o processo do driver) |

**Expectativas** em `expect` (quando: `at`, `during`, `within`, `before`, como na ref/03 §2.5.4; combinadores `any_of`, `all_of`):

| Sujeito | Predicados |
|---|---|
| `call = "<key>"` | `ok` (bool), `failure` (caso de `batch-failure`), `samples` (`"exact"`: uma amostra por ponto do lote), `acks` (lista de `write-status`, pela ordem), `diagnostic` (código), `plan_equal` (bool: os dois planos de `plan_twice` com os mesmos bytes), `link` (estado do link depois da chamada, `link_state` da ref/00 §6) |
| `point = "<ponto do perfil>"` | `equals` (valor cru, ou `"from_fixture"` para o de `[values]`), `quality` (o `raw-status`, com os literais da ref/03 §2.1.3) |
| `wire = "double"` | `no_writes` (bool: o duplo não viu escritas) |
| `driver = "<tier>"` | `trap` (W), `killed` (X), `no_block` (N: nenhum `poll` > 1 ms) |

Uma fixture ou um caso mal formados são `J0887`; um caso falhado é `J0879`.

### 6.4 Casos obrigatórios da v1

O id de cada caso nos relatórios é `<id da suite>.<id do caso>` (`drv.read` + `timeout` = `drv.read.timeout`), como no núcleo e nas classes (ref/03 §2.6.1).

| Id | Nível | Verifica | Requer |
|---|---|---|---|
| `drv.meta.valid` | basic | descritor bem formado, nome, versão, tier, esquemas | — |
| `drv.meta.schema_roundtrip` | full | as chaves do fixture validam pelo esquema e chegam ao driver com os tipos certos | — |
| `drv.plan.pure` | basic | W: plano sem importações além de `log`; N: sem I/O (transporte de teste que falha ao ser usado) | — |
| `drv.plan.deterministic` | basic | dois planos do mesmo pedido, em processos diferentes, com os mesmos bytes | — |
| `drv.plan.coverage` | basic | regras de §2.6 | — |
| `drv.plan.errors` | basic | endereço inválido → `J0810`; sobreposição → `J0811`; `forbidden` → `J0812`; grupo coerente grande demais → `J0813` (os que se aplicam à família) | — |
| `drv.plan.coherent` | basic | pontos coerentes no mesmo lote e lidos numa só operação do duplo | `coherent` |
| `drv.connect.ok`, `drv.connect.refused` | basic | `connect-info` por dispositivo; recusa → erro com código, link `down` | — |
| `drv.read.exact_points` | basic | uma amostra por ponto do lote, nenhuma a mais, `raw-kind` = `expect`, valores do fixture | — |
| `drv.read.timeout` | basic | `timeout` antes do prazo + margem; `degraded` e `down` pelas regras de §2.12 | — |
| `drv.read.device_fault` | basic | exceção do duplo → `device-fault` | — |
| `drv.read.cancel_safe` | basic | leitura cancelada a meio; a seguinte está correta (sem resposta atrasada misturada) | — |
| `drv.quality.allowed` | basic | só pares de §2.8 | — |
| `drv.reconnect` | basic | `close` do duplo → `down` → `disconnect` → `connect` → `up`, sem estado velho | — |
| `drv.write.order_and_acks` | basic | ordem no fio igual à da chamada; um ack por escrita, mesmo `seq` | `write` |
| `drv.write.atomic` | basic | grupo atómico pela ordem, sem intercalação; falha a meio → `skipped` nas seguintes | `write` |
| `drv.write.rejected` | basic | exceção → `device-rejected` | `write` |
| `drv.addr.roundtrip` | basic | `write-addr` seguido de `read-addr` no mesmo endereço | `raw-access` |
| `drv.push.delivery` | basic | amostras `push` entregues com `ts` e sem perdas abaixo do ritmo nominal | `push` |
| `drv.push.cancel_safe` | basic | `read` `push` cancelada para uma `write`; nenhuma mensagem perdida | `push`, `write` |
| `drv.action.native` | full | `invoke` devolve o registo; timeout; `cancelled` numa ação longa | `native-actions` |
| `drv.discover.read_only` | basic | descoberta e scan: o duplo não viu escritas (`J0871`) | `discover` ou `scan` |
| `drv.discover.stable` | basic | duas descobertas seguidas dão o mesmo mapa (depois de recodificado) | `discover` |
| `drv.timed.precision` | full | escritas `at` aplicadas dentro da tolerância declarada; `late` quando impossível | `timed-write` |
| `drv.relinquish` | full | `relinquish` chega ao duplo como libertação | `relinquish` |
| `drv.health` | basic | `health` sem I/O, < 100 µs | — |
| `drv.n.no_block` | basic (N) | nenhum `poll` > 1 ms (`J0883`) | tier N |
| `drv.n.no_std` | basic (N, `micro`) | compila para `thumbv8m.main-none-eabihf` sem `alloc` na parte `Driver` | `micro = true` |
| `drv.w.limits` | basic (W) | respeita `memory` e `cpu_per_call`; trap → `stopped` e recriação | tier W |
| `drv.x.handshake` | basic (X) | `hello` em ≤ 5 s; token errado → recusado; versão | tier X |
| `drv.x.restart` | basic (X) | processo morto → `stopped` → reinício → `resync` das saídas | tier X |
| `drv.x.sandbox` | full (X) | o processo não abre destinos fora de `net` nem ficheiros fora do `StateDirectory` | tier X, `systemd` |

`basic` é obrigatório para entrar no catálogo e para publicar no Hub; `full` é mostrado no Hub como selo. Um caso cuja capacidade o driver não declara fica `skip`, com nota, e não conta.

### 6.5 Relatório

CBOR determinístico (e `--json`): driver, versão, tier, digest do artefacto, versão da suite, fixture (digest), resultado por caso (`pass`, `fail` com o passo e o detalhe, `skip` com a capacidade em falta), métricas (latência p50/p99 de leitura, bytes). O Hub exige o relatório assinado pela chave de publicação, com todos os `basic` a `pass` para a versão publicada (`J0880`). Um caso falhado é `J0879`.

---

## 7. Códigos J0850–J0899

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0850` | error | driver tier N não compilado no runtime do nó |
| `J0851` | error | descritor do driver em falta, mal formado ou incoerente com `[drivers]` |
| `J0852` | error | chave do dispositivo fora do esquema do driver (tipo, gama, valores, obrigatória em falta) |
| `J0853` | error | endereço de ponto fora do esquema de endereço do driver |
| `J0854` | error | o perfil ou o manifesto exige uma capacidade que o driver não declara |
| `J0855` | error | o `plan` do driver falhou sem código válido, ou esgotou o *fuel* |
| `J0856` | error | plano incoerente (cobertura, lotes, períodos, limites) |
| `J0857` | error | plano não determinista: o nó calculou um plano diferente do `jk check` |
| `J0858` | error | o `safe` de uma saída não é representável no tipo cru |
| `J0888` | warning | um limite da `range` de uma saída não é representável no tipo cru (o valor é saturado) |
| `J0859` | error | versão do mundo `joker:driver` não suportada por este runtime |
| `J0860` | error-in-prod | componente de driver usa itens instáveis do mundo (na 1.0 e na 1.1, qualquer driver W de fora da árvore, §4) |
| `J0861` | error | sem versão JDP comum |
| `J0862` | error | autenticação JDP recusada (credenciais do par ou token) |
| `J0863` | error | violação do protocolo JDP; driver terminado |
| `J0864` | error | não foi possível arrancar o driver tier X (executável, utilizador, unidade) |
| `J0865` | error-in-prod | driver tier X sem utilizador próprio ou com grupos privilegiados |
| `J0866` | error | driver em crashloop (limiares por tier no §2.12) |
| `J0867` | warning | chamada ao driver excedeu o prazo |
| `J0868` | error | driver tier W: trap ou limite de recursos excedido |
| `J0869` | warning | amostra inválida do driver (ponto, tipo cru, qualidade, tamanho) |
| `J0870` | warning | anel de ingestão cheio: amostras perdidas |
| `J0871` | error | escrita no equipamento durante descoberta ou scan |
| `J0872` | error | executável do driver tier X inválido para a arquitetura do nó |
| `J0873` | error | transporte não suportado por este host para este tier |
| `J0874` | error | recursos declarados pelo driver acima do permitido |
| `J0875` | error | driver sem suporte `no_std` num alvo Joker Micro |
| `J0876` | warning | ligação ao equipamento falhou (`connect`) |
| `J0877` | warning | escrita recusada ou não suportada pelo equipamento |
| `J0878` | warning | escrita temporizada aplicada fora da tolerância |
| `J0879` | error | caso de conformidade do driver falhou |
| `J0880` | error | publicação de driver sem relatório de conformidade válido |
| `J0881` | error | driver tier X encravado (saúde sem resposta); terminado |
| `J0882` | error | driver tier W usou uma importação proibida em modo plan |
| `J0883` | warning | driver tier N bloqueou a thread `io` mais de 1 ms |
| `J0884` | warning | grupo atómico com saídas em instâncias de driver diferentes: a ordem entre elas não é garantida no fio |
| `J0885` | error | nome, tier, versão ou digest do descritor do driver diferentes do lock, ou plano feito com outro descritor |
| `J0886` | warning | driver acima do ritmo de logs ou de progresso; mensagens descartadas |
| `J0887` | error | fixture ou caso de conformidade de driver mal formado |
| `J0889`–`J0899` | — | reservados |

Em `mode = "dev"`, os `error-in-prod` aparecem como `W0860` e `W0865` (16 §9).

---

## 8. Comportamento esperado

**Semântica.** Um driver não tem semântica própria de qualidade, tempo, envelope nem árbitro: tudo isso é do núcleo (documento 16, §2–§3) e é igual em todos os tiers. O driver só diz o que o equipamento respondeu.

**Determinismo.** O `plan` é puro e determinista (§2.6). As amostras de um driver não são deterministas (vêm do mundo), mas entram no traço pela imagem de processo (16 §7), e por isso o replay não precisa do driver. O driver `sim` é determinista (ref/03 §3).

**Ordem.** Por instância: as escritas saem pela ordem das chamadas e, dentro de uma chamada, pela ordem da lista; os grupos atómicos não são intercalados. Entre instâncias do mesmo barramento, o escalonador serve por prioridade (§2.14). Entre instâncias de barramentos diferentes não há ordem garantida (`J0884` se um grupo atómico as atravessar).

**Concorrência.** Uma chamada por instância de cada vez, com a exceção do `push` (§2.14). Entre instâncias, tudo é concorrente no domínio `io`. O estágio de aquisição é o escritor único dos slots dos sinais do dispositivo (01 §3.2), para os três tiers.

**Falhas.** Um driver N que entre em pânico derruba o `jokerd` (ADR 0016; mitigado pelo supervisor, pelo watchdog e pelo fallback do dispositivo). Um driver W em trap e um driver X morto afetam só as suas instâncias: `stopped`, `bad(not_connected)`, reinício com backoff, `resync` (§4.5, §5.7).

**Gerações.** Uma geração nova só reinicia as instâncias cujo pedido de plano ou artefacto mudou (01 §15, diff mínimo). Um envelope novo não toca no driver. Os `PointId` podem mudar entre gerações; nunca dentro de uma.

**Compatibilidade.** O contrato evolui como o `joker:runtime` (M21): numa minor só se acrescentam operações opcionais, campos no fim dos registos (CBOR: chaves novas; WIT: um mundo novo com adaptador), casos no fim de enums, capacidades e tipos de mensagem. Um driver compilado para a 1.y corre em qualquer 1.x com x ≥ y (21 §1.1).

**Segurança.** Um driver só recebe o transporte que o manifesto lhe dá (W: por capacidade; X: pela sandbox; N: por regra e revisão). Nenhum tier recebe propostas nem pode escrever sem passar pelo envelope: os drivers só veem escritas aplicadas e os passos de ações autorizados (ADR 0005). Os segredos (`secret`) chegam só no `open`, nunca em `plan`, logs ou relatórios.

---

## 9. Estresse

**D1 — Um driver W chama o transporte dentro do `plan`.** No portátil, o `jk check` corre o `plan` em modo plan; a chamada faz trap com `J0882`, e a mensagem diz qual importação foi usada. *O que partiu:* na primeira versão, o `plan` corria na mesma instância que o `connect`, e um driver podia "validar" o endereço fazendo um pedido ao equipamento, o que tornava o `jk check` dependente da rede. **Correção incorporada:** modo plan numa instância própria, só com `log` (§4.4), e o mesmo contrato verificado no N pelo caso `drv.plan.pure`.

**D2 — Dez medidores num RS-485 e um encrava.** Um medidor deixa de responder. Cada lote dele gasta o `timeout` inteiro, e o escalonador serializa o barramento. *O que partiu:* sem regra, o driver fazia as suas próprias repetições dentro do `read`, e um medidor morto com `retries = 3` ocupava 4 × 300 ms por poll, roubando o barramento aos outros nove. **Correção incorporada:** uma tentativa por chamada, repetições do núcleo só se couberem no período (§2.14), e o dispositivo passa a `down` ao fim de 3 falhas totais (§2.12), com o poll a passar ao ritmo do backoff. O orçamento `J0370` já conta o pior caso de um timeout por ciclo.

**D3 — Um driver X morre a meio de um grupo atómico.** O `WRITE` de `pcs.mode` saiu, o de `pcs.p_set` não. Os acks nunca chegam. O `jokerd` marca `not-connected` nas duas escritas, o grupo fica `uncertain(partial_write)` (02 §7.3), os pontos `bad(not_connected)`. Reinício em 1 s; depois do `connect`, o `resync` reenvia o grupo inteiro pela ordem. **Aguenta.** O intervalo sem controlo é coberto pelo fallback do equipamento, como num driver N em baixo.

**D4 — Um SDK de fabricante com um buffer overflow escreve lixo na memória partilhada.** O processo escreve `up.head = 2⁶³` e bytes aleatórios nas entradas. *O que partiria* com uma memfd única de escrita partilhada: o driver podia também reescrever os pedidos `down` ou o `tail` do `jokerd`, e o `jokerd` podia ler uma entrada a mudar enquanto a interpretava (TOCTOU). **Correção incorporada:** duas memfd com um só escritor cada, a do `jokerd` selada com `F_SEAL_FUTURE_WRITE` (§5.5.1), cópia antes de validar e verificação dos índices (§5.5.6). O `head` impossível é `J0863`: SIGKILL e reinício.

**D5 — Outro driver X tenta falar no socket de um vizinho.** O processo de `acme-bms` liga-se a `/run/joker/drv/bacnet-ip.sock` (o grupo `joker-drv` dá-lhe acesso ao diretório). O `SO_PEERCRED` mostra um pid que não pertence ao cgroup `joker-drv-bacnet-ip.service`: `J0862`, ligação fechada. Mesmo que passasse, faltava-lhe o token. **Aguenta**, com duas verificações independentes (§5.2.3).

**D6 — O `jokerd` não é root e o driver tem de correr com outro utilizador.** *O que partiu:* o documento 01 §17 diz ao mesmo tempo que o `jokerd` não precisa de root e que os tier X correm com utilizador próprio; um processo sem `CAP_SETUID` não consegue arrancar outro com outro utilizador. **Correção incorporada:** arranque por unidades transitórias do systemd com `DynamicUser` e uma regra polkit limitada a `joker-drv-*` (§5.2.1); `spawn` só com `CAP_SETUID` explícito, e `J0865` em `prod` sem utilizador próprio.

**D7 — Um driver MQTT (push) com uma escrita pendente.** A leitura `push` está à espera de mensagens; chega uma escrita. O núcleo cancela a leitura, faz a escrita, e arma a leitura de novo. *O que partiu:* um driver que tinha retirado uma mensagem do cliente MQTT, mas ainda não a tinha entregado, perdia-a no cancelamento. **Correção incorporada:** leituras `push` têm de ser *cancel-safe* (§2.14), e o caso `drv.push.cancel_safe` apanha-o. No tier X o problema não existe (o processo é concorrente; §5.5.4).

**D8 — Um driver N chama uma biblioteca C bloqueante.** Uma chamada de 40 ms prende uma thread `io` do tokio, e os lotes de outros dispositivos nessa thread atrasam-se. O caso `drv.n.no_block` mede cada `poll` e falha acima de 1 ms (`J0883`). A saída é `cx.spawn_blocking` (com prazo) ou tier X. **Aguenta**, com a regra escrita e testada (§3.5).

**D9 — O mesmo driver em três tiers.** Um integrador escreve um driver `acme-bms` em Rust, testa-o como N em `cargo test`, publica-o como X (tem de ligar uma biblioteca do fabricante) e, quando o fabricante abre o protocolo, como W. Os três relatórios de conformidade dão os mesmos resultados; muda só a latência. **Aguenta**, porque os tipos, as operações e os códigos são os mesmos (§3.1), e a suite é uma só (§6).

**D10 — Versões cruzadas.** Um nó 1.3 recebe um driver W compilado para `joker:driver@1.2.0` (aceite, pelo adaptador do minor anterior) e outro para `@1.4.0` (recusado pelo agente antes do deploy, `J0859`). Um driver X com `libjdp` 1.5 fala JDP 1 com um `jokerd` 1.0, porque o `welcome` escolhe a versão. *O que partiu:* o 05 §8 importava `joker:runtime/log@0.3.0` num `joker:driver@0.1.0`, o que tornava impossível dizer a que runtime um driver se destina. **Correção incorporada:** as duas versões são sempre iguais (§4.1).

**D11 — Um driver W lento no CPU.** Um driver DLMS faz AES por software e gasta 30 ms por leitura. Cede a thread a cada 1 ms (epoch com *yield*), por isso não prende uma thread `io`; soma 30 ms, abaixo dos 50 ms de `cpu_per_call`. Com 80 ms, faz trap (`J0868`) e o integrador sobe `cpu_per_call` no `[drivers]` ou reescreve o driver. **Aguenta**, com o limite visível e ajustável.

**D12 — O driver do Joker Micro com o plano no portátil.** O `jk build --target mcu:rp2350` corre o `plan` do Modbus RTU no portátil, e o plano vai para o blob. O firmware no campo é de outra versão do driver (uma patch). *O que partiria:* um plano feito com a versão X executado pela versão Y, com `private` num formato diferente. **Correção incorporada:** o blob leva o digest do descritor com que o plano foi feito, e o firmware recusa um plano de outro descritor (`J0885`; o `HELLO` do Joker Link reporta-o, ref/07). Mudar o driver do firmware obriga a recompilar a geração.

---

## 10. Decisões e alternativas rejeitadas

| Decisão | Alternativas rejeitadas | Porquê |
|---|---|---|
| **Um contrato, três hosts**; o mesmo trait Rust serve N, W (SDK) e X (`joker-jdp`) | três APIs desenhadas à parte | um driver muda de tier sem ser reescrito; uma suite só |
| `plan` separado do runtime do driver (`Planner` com `alloc`, `Driver` `no_std`) | um trait único com `plan` | o plano corre no portátil e no build do MCU; o firmware não precisa de `alloc` |
| Passos de ações, `[connect]`, `[relinquish]` e fallback executados **pelo núcleo** sobre `read-addr`/`write-addr` | cada driver implementa passos e M1 | a semântica das ações e do fallback fica num sítio; um driver novo ganha-as de graça |
| Codificação cru ↔ engenharia no núcleo, incluindo `word_order` e arredondamento `round_ties_even` | o driver devolve já o valor escalado | 05 §2 e ADR 0003: uma implementação, dois drivers não divergem; determinismo entre arquiteturas |
| Repetições e backoff no núcleo; uma tentativa por chamada | repetições dentro do driver | orçamento do barramento previsível (D2) |
| Push como leitura longa cancel-safe | `subscribe` com tarefa própria do driver | funciona em `no_std` (sem spawn dinâmico) e com `&mut self`; no X, push natural por `jdp_push` |
| Configuração e endereços em CBOR **com esquema declarado**; o resto em WIT | tudo em WIT (um tipo por driver) ou tudo em bytes | o que é genérico fica tipado; o que é do protocolo é validado pelo núcleo com mensagens iguais em todos os drivers |
| JDP em `SOCK_SEQPACKET` com CBOR, e dados em anéis SPSC de entradas de 64 B | gRPC, protobuf, stream com prefixo de comprimento, anel único | CBOR é a codificação do projeto (ref/00 §7); seqpacket preserva fronteiras e leva fds; 64 B = uma linha de cache |
| Duas memfd com um escritor cada, uma selada `F_SEAL_FUTURE_WRITE` | uma memfd partilhada | o driver não consegue corromper o `jokerd` (D4) |
| Arranque por unidades transitórias do systemd com `DynamicUser` | `jokerd` com root ou com `CAP_SETUID` por omissão | 01 §17 sem contradição (D6); sandbox com as ferramentas do SO |
| Um processo X por driver por omissão (`per_device` opcional) | um processo por dispositivo sempre | os SDKs de fabricante são muitas vezes singletons; o isolamento fino fica disponível |
| Estado `stopped` e motivo `not_connected` para driver morto; `comm_failure` para equipamento que não responde | um só motivo | distingue "o software caiu" de "o cabo caiu", que têm reações diferentes |
| Interfaces WIT `guest`/`host`/`transport` (o nome `driver` ficou para o mundo) | interface `driver` | WIT não admite uma interface e um mundo com o mesmo nome |

---

## 11. Marcações de versão

| Item | Versão |
|---|---|
| contrato comum (§2), capacidades exceto `block-read` e `streams` | **1.0** |
| tier N: `joker-hal`, registo por features, `rt-cycle`, `no_std` no Joker Micro | **1.0** |
| tier X: JDP 1 completo (controlo, anéis, supervisão, sandbox), `libjdp`, `joker-jdp` | **1.0** |
| tier W: mundo `joker:driver` (versão fixada com o `joker:runtime`) e `joker-driver-sdk` **instáveis** (`@unstable(feature = driver-world)`, só drivers da árvore em `prod`, §4); host com `serial`, `tcp`, `udp` | **1.0** |
| tier W **estável e aberto a terceiros** (`@since(1.2.0)`): mundo, SDK Rust e C, transportes `i2c`, `spi`, `can` no host (`w-i2c-spi-can`), conformidade obrigatória, publicação no Hub | **1.2** (21 §4) |
| suite de conformidade: interface, casos `basic`, relatório | **1.0** (usada pelo catálogo); obrigatória para publicar no Hub na **1.2** |
| streams de drivers X por `SCM_RIGHTS` (mensagens 40–42), capacidade `streams` | **1.1** (M4; reservado já no CDDL, "Preparado para ML") |
| sinais em bloco, capacidade `block-read` | **1.1** (M7; reservado) |
| Wasm no Joker Micro (drivers W com `wasmi`) | **1.4** (21 §6); o mundo é o mesmo |
| drivers de edifícios e energia sobre este contrato (`bacnet-ip` X, `dlms-cosem` W, `iec-104` X) | **1.2** no catálogo (BACnet em qualquer tier, 21 §4); depois, novos drivers em qualquer minor (ADR 0022, regra 3) |
