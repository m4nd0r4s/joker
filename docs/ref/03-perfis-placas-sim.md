# ref/03 — Perfis, placas, simulação, cenários, conformidade e pacotes

Referência completa dos ficheiros TOML de **catálogo** (perfis de dispositivo, placas, pacotes de classes) e de **teste** (modelos de simulação, cenários, casos de conformidade do núcleo e das classes), e da forma como os artefactos de catálogo se publicam no Hub. Aplica o `ref/00` sem o repetir. A semântica (árbitro, envelope, qualidade, expressões) é a do documento 16; aqui fica a representação e a validação.

---

## 1. Âmbito e fontes

| Superfície | Ficheiro | Deriva de | Liga-se a |
|---|---|---|---|
| perfil de dispositivo | `profiles/<fabricante>/<modelo>@<major>.profile.toml` | 05 §3–§7, §12; 02 §2, §4, §6.2, §8; 01 §3.3, §8; 14 §5.1; 17 §5 | ref/01 (`[devices.*]`), ref/02 (lock), ref/06 (`plan` dos drivers) |
| placa | `boards/<placa>.board.toml` | 05 §11; 01 §2, §22; 19 §3–§4 | ref/01 (`[target]`, `[resources]`), ref/08 (capacidades do firmware) |
| modelo de simulação | `sim/<modelo>.sim.toml`, `classes/<pacote>/<nome>.sim.toml` | 04 §6; 14 A13; trilha 1, 3–5 | ref/01 (`driver = "sim"`, `sim =`), ref/04 (mundo `joker:sim`) |
| cenário | `sim/scenarios/<nome>.scenario.toml` | 04 §6; exemplos 00–02; trilha 1, 3–5, 8 | ref/01 (manifesto sob teste), 07 §8 (código de saída 6) |
| conformidade do núcleo | `conformance/<área>/<suite>.conformance.toml` | 16 §8; 13 §3 | 16 (todas as regras), ref/08 (execução no MCU) |
| conformidade de classe | `classes/<pacote>/<nome>.conformance.toml` | 14 §7.3; 17 §4 | ref/04 (`joker-test`) |
| pacote de classes | `classes/<pacote>/<pacote>.package.toml` | 17 §3.2, §9; 20 §3.2 | ref/02 (lock), ref/10 (artefactos) |
| artefactos OCI | registo (Hub) | 20 §3.2; ref/00 §7.1 | ref/09, ref/10 |

O manifesto (`joker.toml`) e o `joker.lock` não são desta referência: aqui só se diz o que eles **referem** e o que o lock tem de fixar (§3 Dependências, no fim de cada secção).

---

## 2. A referência

### 2.1 Regras comuns a todos os ficheiros

#### 2.1.1 Cabeçalho

| Chave (raiz) | Tipo | Obrigatória | Omissão | Validação |
|---|---|---|---|---|
| `schema` | inteiro | sim | — | tem de ser o **primeiro par chave/valor** do ficheiro; antes dele só comentários e linhas vazias (`J0800`); valor `1` na v1 (`J0801`). É a regra do ref/00 §1 (D11) |
| `joker` | string `"M.m"` | não | `"1.0"` | versão mínima da runtime que lê o ficheiro; uma runtime mais antiga recusa-o com `J0802`, e não com `J0140` |

- O **tipo** do ficheiro (perfil, placa, modelo, cenário, suite, fixture, pacote) vem do **sufixo do nome** (`.profile.toml`, `.board.toml`, …; ADR 0026) e, num artefacto OCI, do tipo de media (§2.8). O conteúdo tem de bater com o sufixo, e quem refere o ficheiro tem de pedir esse tipo: as duas verificações e o `J0803` estão na §2.1.6.
- Cada tipo com nome publicável, e a fixture de drivers, tem uma tabela de identificação obrigatória: `[profile]`, `[board]`, `[package]`, `[scenario]`, `[suite]`, `[fixture]`. O modelo de simulação não tem nome publicável, e a tabela `[model]` é opcional; reconhece-se pelas chaves da §2.1.6.
- **Estrito** (ref/00 §1): chave desconhecida é `J0140`; chaves começadas por `x_` são ignoradas em qualquer nível; chaves começadas por `_` são reservadas (`J0141`, como no manifesto).
- Codificação: TOML 1.0, UTF-8, sem BOM. Tamanho máximo de um ficheiro: 4 MiB.

#### 2.1.2 Nomes

| Coisa | Gramática | Exemplo | Erro |
|---|---|---|---|
| nome de perfil | `seg "/" seg`, com `seg := [a-z0-9]+ ("-" [a-z0-9]+)*`, ≤ 64 caracteres no total | `eastron/sdm630`, `acme/pcs-125`, `local/xy-md02` | `J0805` |
| referência a perfil | `<nome>@<major>` | `acme/pcs-125@3` | `J0805` |
| nome de placa | `seg` (mesma gramática), ≤ 48 caracteres | `rpi5`, `revpi-connect-5`, `rp2350-pico2` | `J0805` |
| pacote de classes | `ident ("/" ident)*` (documento 17, §3.2), `ident := [a-z][a-z0-9_]*` | `std`, `acme/water` | `J0805` |
| caminho de sinal relativo (perfil, modelo) | o de 02 §9, sem o segmento do dispositivo: no máximo 7 segmentos | `l1.v`, `status.fault` | `J0805` |
| nome de enum | `[A-Z][A-Za-z0-9]*`, ≤ 32 | `PcsState` | `J0805` |
| variante, bit, ação, argumento, parâmetro, grupo coerente | `[a-z][a-z0-9_]*`, ≤ 32 | `p_control`, `reset_fault` | `J0805` |
| id de caso e de suite | `[a-z0-9_]+ ("." [a-z0-9_]+)*`, ≤ 96 | `envelope.rate.clamp_then_safe` | `J0805` |

**Espaços de nomes reservados:** `joker/` (perfis gerados ou embutidos na runtime: `joker/sunspec@1`, perfis do Joker Link); `local/` (perfis e pacotes do projeto, que **nunca** se publicam: `J0848`); `std` (pacote de classes padrão, só publicado pelo projeto). Um nome dentro de um espaço reservado fora do seu sítio é `J0805`.

**O ficheiro e o nome concordam:** `profiles/eastron/sdm630@1.profile.toml` tem `name = "eastron/sdm630"` e `version = "1.x.y"`; `boards/rpi5.board.toml` tem `name = "rpi5"`; `classes/acme/water/water.package.toml` tem `name = "acme/water"`. Divergência é `J0804`.

#### 2.1.3 Literais

Todos os literais seguem o ref/00 §3 e §4. Um literal mal formado é `J0806`.

| Literal | Gramática | Exemplos | Onde |
|---|---|---|---|
| duração | ref/00 §3.1 | `"500ms"`, `"1.5s"`, `"10m"` | chaves de duração |
| quantidade | ref/00 §3.2; unidade do catálogo (16 §5, `J0109`) | `"3.3 V"`, `"-20 kW/s"`, `"5 %"` | valores físicos |
| número nu | inteiro ou decimal TOML | `1.65`, `0x48` | valor **na unidade declarada** do sinal |
| instante (linha temporal) | `duração ("+" duração)*`, espaços opcionais à volta de `+`; nas suites de drivers, um dos termos pode ser `timeout` (§5, ref/06 §6.3) | `"3m"`, `"2m+1.2s"`, `"100ms + timeout + 50ms"` | `at`, `before`, extremos de intervalo — **só** em cenários e casos (ref/00 §3.1) |
| intervalo | `instante ".." instante`, fechado nos dois extremos, com `início ≤ fim` | `"2s .. 5s"` | `during`, `within` |
| tolerância | `duração ("±" \| "+-") duração` | `"500ms ± 20ms"` | `toggles_every` |
| qualidade | `código` \| `código "(" motivo ")"`, nomes de ref/00 §4.3 em `snake_case` | `"good"`, `"bad(comm_failure)"`, `"uncertain(stale)"` | `quality` |
| resultado de escrita | `resultado` \| `"rejected(" motivo ")"` \| `"overridden(" nível ")"`, nomes de ref/00 §4.4 | `"clamped_rate"`, `"rejected(interlock)"`, `"overridden(manual:4)"` | `outcome` |
| estado de alarme | ref/00 §6 `alarm_state` | `"active_unacked"`, `"unknown"` | `state` |
| nível do árbitro | ref/00 §5, ou `"none"` | `"optimizer:13"` | `level` |
| valor de enum | `"variante"` ou `"Enum.variante"` | `"fault"`, `"StatusMode.off"` | valores de sinais enum |

**Formas de apresentação aceites** (ref/00 §2; D10). A CLI mostra `Bad(CommFailure)`, `Clamped(Rate)`, `Rejected(Interlock)`, `Unknown`, e é natural copiá-las para um ficheiro. Por isso, em `quality`, `outcome` e `state`, a forma de apresentação é aceite como sinónimo exato da canónica, com esta tradução: `PascalCase` → `snake_case`, `Clamped(Range|Rate)` → `clamped_range|clamped_rate`, `Overridden(nível)` → `overridden(nível)`. É uma tradução total e sem ambiguidade; o `jk fmt` reescreve para a forma canónica. Qualquer outra grafia é `J0806`.

**Unidades nos valores.** Um valor com unidade é convertido para a unidade do sinal (dimensão diferente: `J0107`; grandeza diferente: `J0108`). Um número nu está na unidade do sinal. `%` não se converte em `1` nem ao contrário (02 §3).

#### 2.1.4 Seletores

Onde se aceita um caminho de sinal num cenário ou num caso, aceita-se um seletor de 02 §9 (`*` num segmento, `**` em vários). Um seletor expande-se no `jk check` para a lista ordenada de sinais que o casam; zero correspondências é `J0841`. A semântica de uma expectativa com seletor é "**todos** os sinais casados cumprem", salvo indicação em contrário.

#### 2.1.5 Limites da v1

| Coisa | Limite |
|---|---|
| sinais por perfil | 4 096 |
| sinais por modelo de simulação | 1 024 |
| variantes por enum | 256 |
| bits nomeados por sinal | 64 |
| passos por ação | 32 |
| grupos coerentes por perfil | 64; 125 sinais por grupo |
| entradas em `[implements]` | 16 classes por perfil ou modelo |
| ações e expectativas por cenário | 4 096 cada |
| duração de um cenário | 30 d de tempo virtual |
| casos por suite | 1 000 |
| `delay` e `lag` na simulação | 15 min (como as janelas, `J0305`) |

#### 2.1.6 O tipo está no nome

A regra geral dos nomes de ficheiros está no ref/00 §2 (ADR 0026). Nesta referência:

| Sufixo | Tipo | Reconhece-se pelo conteúdo por | Quem o refere |
|---|---|---|---|
| `.profile.toml` | perfil de dispositivo (§2.2) | `[profile]` | `profile = "<nome>@<major>"` (resolução pelo nome, §3), `[model] profile`, `[fixture] profile`, `jk profile …` |
| `.board.toml` | placa (§2.3) | `[board]` | `[target] board = "<placa>"` (resolução pelo nome), `jk board …` |
| `.sim.toml` | modelo de simulação (§2.4) | pelo menos uma de `[model]`, `[signals]`, `[plant]`, e nenhuma tabela de identificação de outro tipo | `model = "<caminho>"`, simulador de classe (§2.4.11) |
| `.scenario.toml` | cenário (§2.5) | `[scenario]` | `jk test [<caminho>…]`, pasta `sim/scenarios/` |
| `.conformance.toml` | suite de conformidade (§2.6) | `[suite]` | `jk class verify`, `jk driver test`, executor do núcleo |
| `.fixture.toml` | fixture de drivers (ref/06 §6.2) | `[fixture]` | `jk driver test --fixture <caminho>` |
| `.driver.toml` | descritor de driver em TOML (ref/06 §2.3) | `name`, `tier` e `protocols` na raiz | `jk driver meta` |
| `.package.toml` | pacote de classes (§2.7) | `[package]` | `jkc`, `jk class publish` (um por pasta de pacote) |

**Verificação conteúdo ↔ sufixo (`J0803`).** Antes da validação das chaves (§3, "Validação"), o `jk check` lê as tabelas de topo de cada ficheiro e confirma que batem com o sufixo. É `J0803`:

- um ficheiro sem a tabela de identificação do seu sufixo (um `.board.toml` sem `[board]`), ou um `.sim.toml` sem nenhuma das chaves que o caracterizam;
- um ficheiro com a tabela de identificação de **outro** tipo (um `.sim.toml` com `[profile]`), ou de dois tipos;
- um ficheiro referido por quem pede outro tipo (`model = "profiles/local/xy-md02@1.profile.toml"`; `jk test boards/rpi5.board.toml`);
- um ficheiro TOML de uma destas pastas (`profiles/`, `boards/`, `sim/`, `conformance/`, `classes/`) sem nenhum dos sufixos da tabela. O `jk migrate` renomeia-o (documento 07, §2).

A mensagem diz o tipo esperado pelo sufixo e o tipo que o conteúdo mostra ("`sim/esp.sim.toml` tem `[profile]`: é um perfil; um perfil chama-se `<modelo>@<major>.profile.toml` e vive em `profiles/`"). O `J0803` vem antes do `J0140`: um perfil com o nome de uma simulação daria, de outro modo, uma lista de chaves desconhecidas.

**Resolução pelo nome.** Os perfis e as placas referem-se pelo nome, sem sufixo; a resolução acrescenta-o (§3): `local/bench-esp32@1` → `profiles/local/bench-esp32@1.profile.toml`; `rpi-zero-w` → `boards/rpi-zero-w.board.toml`. Os caminhos de ficheiro (`model`, `manifest`, `--fixture`) escrevem-se inteiros, com o sufixo.

**Editores.** O `jk init` e o `jk new` escrevem na raiz do projeto um `.taplo.toml` que associa cada sufixo (e `joker*.toml`) ao esquema JSON do seu tipo (gerado dos tipos Rust, como o do manifesto, e publicado com cada versão da `jk`); o `jk migrate` acrescenta-o a um projeto antigo. O ficheiro versiona-se, e a `jk` não o reescreve se já existir.

---

### 2.2 Perfis de dispositivo

Um perfil descreve um **modelo** de equipamento (15 §1). O driver entrega valores crus; o estágio de aquisição do núcleo aplica, a partir do perfil, tipo cru, byte order, escala, offset, unidade, limites e deteção de congelamento (01 §3.3). O `plan` do driver (05 §2) valida o que é específico do protocolo; o `jk check` valida tudo o resto sem I/O.

#### 2.2.1 Estrutura

```toml
schema = 1

[profile]            # identificação, protocolo, quirks
[defaults]           # valores por omissão dos pontos
[signals]            # pontos: um por sinal
[enums]              # enums nomeados
[coherent]           # grupos lidos no mesmo pedido
[config]             # esquema da `config` do dispositivo no manifesto
[fallback]           # M1: fallback do próprio equipamento
[actions]            # ações (M19)
[connect]            # passos no arranque da ligação
[relinquish]         # passos de `safe = "relinquish"`
[events]             # eventos do dispositivo
[implements]         # classes cumpridas (instâncias implícitas)
[discovery]          # descoberta (SunSpec)
```

Todas as secções são opcionais exceto `[profile]` e `[signals]` (esta pode estar vazia só num perfil de descoberta).

#### 2.2.2 `[profile]`

| Chave | Tipo | Obrig. | Omissão | Validação e significado |
|---|---|---|---|---|
| `name` | string | sim | — | §2.1.2; igual ao caminho (`J0804`) |
| `version` | SemVer | sim | — | major igual ao `@major` do ficheiro (`J0804`) |
| `driver` | string | sim | — | família de drivers (tabela abaixo); o `driver` do dispositivo tem de pertencer à família (`J0808`) |
| `vendor` | string ≤ 64 B | não | — | fabricante, texto livre |
| `model` | string ≤ 64 B | não | — | designação comercial |
| `description` | string ≤ 1 KiB | não | — | |
| `docs` | URL | não | — | ficha técnica |
| `license` | expressão SPDX | não (sim para publicar, `J0848`) | — | licença do ficheiro de perfil |
| `authors` | lista de strings | não | `[]` | |
| `firmware` | string de restrição SemVer (`">=4.10, <5"`) ou lista de strings exatas | não | sem restrição | comparada com o firmware lido no `connect`; fora da gama: `J0826` (aviso em `dev`, erro em `prod`) |
| `identity` | tabela `{ firmware = "<ponto>" }` | não | `{}` | de onde vem a identidade num equipamento **sem descoberta** (um Modbus sem SunSpec): `firmware` é o nome de um ponto de `[signals]` deste perfil, com `direction = "input"` ou `"param"` (um ponto inexistente, ou uma saída, é `J0825`). O valor lido passa a texto (uma `string` tal como está; um inteiro em decimal; com `enum`, o rótulo) e é o `firmware` do dispositivo: o `jk lock --node` escreve-o em `[discovered.<d>] firmware` (ref/02 §2.1.11), a restrição `firmware` acima (`J0826`) compara-o, e a deriva `J0401 kind=firmware` (ref/02 §3.1.5) funciona sem `discover = true`. Outra chave em `identity` é `J0140` |
| `quirks` | tabela | não | `{}` | ajustes do protocolo, por família (§2.2.6); chave desconhecida para a família: `J0818` |

**Famílias de drivers** (catálogo de 05 §10; as chaves de endereço estão em §2.2.5):

| `driver` no perfil | `driver` do dispositivo aceite | Versão |
|---|---|---|
| `modbus` | `modbus-tcp`, `modbus-rtu`; num MCU, `modbus-rtu` (mestre) e `modbus-slave` (o mapa que o MCU serve) | 1.0 |
| `i2c` | `i2c` | 1.0 |
| `spi` | `spi` | 1.0 (`spi-adc` com blocos: 1.1) |
| `gpio` | `gpio` | 1.0 (perfis de expansores; as linhas de SoC vêm da placa) |
| `revpi` | `revpi` | 1.0 |
| `mqtt` | `mqtt` | 1.0 |
| `opcua` | `opcua` | 1.0 |
| `canopen` | `canopen`; `can` para perfis de tramas CAN simples | 1.0 |
| `mbus` | `m-bus` | 1.0 |
| `onewire`, `serial-line` | idem | 1.0 |
| `ethercat`, `bacnet`, `iec104`, `iec61850`, `dlms` | idem, `x:bacnet-ip` | reservado 1.2 (`J0807`) |
| `x:<nome>`, `w:<nome>` | o driver externo ou Wasm com esse nome | 1.0 (chaves de endereço no esquema `address` do descritor, validadas pelo `plan`) |
| `joker/link-*` (gerados, §2.2.16) | `joker-link` | 1.0 (endereçamento por `SignalId` do MCU; não se escrevem à mão) |

Um dispositivo cujo `driver` não é aceite pela família do perfil é `J0808`. O `driver` do perfil tem de estar em `protocols` do descritor do driver (ref/06 §2.3).

#### 2.2.3 `[defaults]`

Qualquer chave de ponto da tabela de §2.2.4 marcada "sim" na coluna *omissão* pode ter um valor por omissão aqui. Um ponto que declare a chave vence. Chave não admitida em `[defaults]`: `J0140`.

#### 2.2.4 `[signals]`: chaves de um ponto

A chave de cada entrada é o caminho relativo do sinal (entre aspas quando tem pontos). O caminho completo no site é `<dispositivo>.<caminho>`. O primeiro segmento `actions` é reservado (`J0805`), porque `actions.<nome>` é uma referência a uma ação nos mapeamentos (§2.2.13).

| Chave | Tipo | Obrig. | Omissão | Em `[defaults]` | Validação e significado |
|---|---|---|---|---|---|
| endereço | ver §2.2.5 | sim | — | parcial | depende da família |
| `type` | tipo cru (§2.2.7) | sim | — | sim | `J0809` se inválido para o endereço |
| `signal_type` | `bool` `i64` `u64` `f64` `f32` | não | derivado (§2.2.7) | sim | só para forçar `f32` ou inteiro; `J0815` se incompatível |
| `direction` | `input` \| `output` \| `param` | não | `input` | sim | `output` exige endereço de escrita (`J0810`); saídas precisam de envelope no manifesto (`J0201`) |
| `unit` | unidade | não (sim para grandezas físicas) | `1` | sim | catálogo 16 §5 (`J0109`); proibida em `bool`, `enum`, `bits`, `string`, `bytes` (`J0815`) |
| `quantity` | grandeza de 16 §5 | não | a da unidade | não | `J0108` se incompatível com a unidade |
| `scale` | número ≠ 0, ou `{ from = [a, b], to = [c, d] }` | não | `1` | sim | valor = cru × `scale` + `offset`; a forma de dois pontos: `from` em unidades cruas, `to` em quantidades; `J0815` em `bool`, `enum`, `bits` |
| `offset` | número (na unidade do sinal) ou quantidade | não | `0` | sim | não se combina com a forma de dois pontos (`J0815`) |
| `scale_factor` | caminho de outro ponto | não | — | não | escala = 10^(valor inteiro do ponto indicado) (SunSpec `*_SF`); o ponto é **automaticamente coerente** com este (§2.2.9); multiplica com `scale` |
| `enum` | nome em `[enums]` | não | — | não | tipo do sinal `enum<Nome>`; `J0814` se não existir |
| `bits` | tabela `índice → nome` | não | — | não | só em `bits16`, `bits32`, `bits64`; índice 0 = bit menos significativo; cada nome cria o sinal `bool` derivado `<caminho>.<nome>` com o mesmo `acq_id` (§2.2.8) |
| `bit` | inteiro | não | — | não | extrai um só bit de um registo como `bool` |
| `invert` | bool | não | `false` | não | só em `bool` (lógica invertida) |
| `len` | inteiro | cond. | — | não | obrigatório em `string` e `bytes` (em registos ou bytes, conforme a família); ≤ 64 B depois de descodificado |
| `encoding` | `ascii` \| `utf8` | não | `ascii` | sim | `string` |
| `invalid` | lista de valores crus | não | `[]` | sim | um cru nesta lista dá `bad(nan)` ("não implementado", `0x8000` no SunSpec) |
| `poll` | duração ou `"on_demand"` | não | o `poll` do dispositivo | não | `"on_demand"` só com `direction = "param"` (`J0817`); `param` sem `poll` é `"on_demand"` |
| `max_age` | duração | não | 3 × `poll` (01 §3.3) | sim | `J0816` se ≤ `poll` |
| `limits` | `[mín, máx]` (quantidades ou números) | não | — | não | limites **físicos**: fora deles, `bad(out_of_range)` (02 §4); `mín < máx` (`J0816`) |
| `stuck_detect` | duração | não | — | sim | `uncertain(frozen)` quando o valor fica bit a bit igual durante esse tempo (02 §4) |
| `deadband` | quantidade | não | — | não | deadband de **aquisição** (só entradas; nas saídas o deadband é do envelope, no manifesto) |
| `monotonic` | bool | não | `false` | não | contador (energia, horas): o historian agrega pelo último valor e não pela média; um decréscimo não muda a qualidade e é contado na métrica `joker_counter_resets_total{signal}` |
| `readback` | caminho de outro ponto | não | — | não | só em `output`: sugere o `verify.readback` do manifesto (o `jk check` mostra a sugestão) |
| `safe_hint` | valor | não | — | não | só em `output`: sugestão para o `safe`, mostrada ao lado de `J0201`; nunca é aplicada |
| `range_hint` | `[mín, máx]` | não | — | não | só em `output`: gama nominal do equipamento, mostrada no `jk check` |
| `label` | string ≤ 64 B | não | — | não | para HMI e `jk read` |
| `description` | string ≤ 1 KiB | não | — | não | |
| `kind` | `scalar` \| `block` \| `stream` | não | `scalar` | não | `block` e `stream` reservados para 1.1 (`J0807`) |

Os ajustes do manifesto (`[devices.<d>.signals]`, `[signals."<caminho>"]`) sobrepõem, num ponto de perfil, só `alias`, `max_age`, `limits`, `stuck_detect`, `deadband`, `debounce`, `scale`, `offset` (aplicados depois dos do perfil), `unit` (da mesma dimensão), `poll` e `record` (ref/01 §5.8.1, com a mesma gramática: a `scale` de dois pontos é `{ from, to }` nos dois sítios); os restantes são do perfil. Nenhum destes chega ao driver: são do estágio de aquisição (ref/06 §2.4).

#### 2.2.5 Endereçamento por família

**Chaves genéricas e chaves de endereço.** As chaves de um ponto dividem-se em duas famílias, validadas por quem as entende:

| Chaves | Quem as define | Quem as valida | Erros |
|---|---|---|---|
| genéricas (§2.2.4: `type`, `direction`, `unit`, `scale`, `limits`, `enum`, `poll`, …) | esta referência, iguais para todos os drivers | o núcleo, no `jk check`, sem o driver | `J0809`, `J0814`–`J0817` |
| de endereço (esta secção) | o esquema `address` do descritor do driver (ref/06 §2.3); para os drivers do catálogo, as tabelas abaixo | primeiro o núcleo, pelo esquema (tipo, gama, valores: `J0853`); depois o `plan` do driver, pela coerência do protocolo | `J0809`–`J0813` |
| `quirks` (§2.2.6) | o esquema `quirks` do descritor | idem | `J0818` |
| `native` das ações (§2.2.12) | o esquema `native` do descritor | idem | `J0821` |

Uma chave que não é genérica nem do esquema de endereço do driver é `J0140`. O `plan` só devolve códigos desta referência (§2.9) ou da ref/06 (`J085x`); um código de outra gama, ou nenhum, é `J0855` (ref/06 §2.6). Assim, um driver tier X cujo executável não corre no portátil continua a ter as suas chaves validadas no `jk check`.

**Modbus** (completo na 1.0):

| Chave | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| `at` | inteiro 0–65535 | sim | — | endereço **do protocolo, base 0** (trilha 4, §4.1: "30001" é o input register 0); `at + registos − 1 ≤ 65535` (`J0810`) |
| `fc` | 1, 2, 3, 4, 5, 6, 15, 16 | sim (em `[defaults]` ou no ponto) | — | entradas e `param`: 1 (coils), 2 (discrete inputs), 3 (holding), 4 (input registers); saídas: 5, 6, 15, 16 (`J0810`) |
| `word_order` | `big` \| `little` | não | `big` | tipos de mais de um registo |
| `byte_order` | `big` \| `little` | não | `big` | dentro de cada registo |
| `bit` | 0–15 | não | — | só com FC 3/4 e `type = "bool"`: extrai um bit de um registo |

- FC 1/2 e FC 5/15 só aceitam `type = "bool"` (`J0809`). FC 6 só aceita tipos de um registo (`J0809`); tipos de 2 ou 4 registos numa saída exigem FC 16.
- **Leitura de uma saída.** Uma saída não é lida em ciclo: o seu slot tem o valor aplicado. O retorno lê-se por um ponto de entrada no mesmo endereço (`p_set_rb` com FC 3), que é o único caso de sobreposição permitido além de pontos que leem exatamente os mesmos registos (bits de um registo, `bit` diferentes).
- Qualquer outra sobreposição de registos entre pontos do mesmo espaço (FC de leitura) é `J0811`.

**I²C e SPI:**

| Chave | Tipo | Obrig. | Validação |
|---|---|---|---|
| `reg` | inteiro 0–65535 | um de `reg` ou `channel` | registo do chip |
| `len` | 1–64 bytes | com `reg` | |
| `byte_order` | `big` \| `little` | não (omissão `big`) | |
| `channel` | inteiro 0–255 | um de `reg` ou `channel` | canal de um *chip handler* do driver (`[profile] quirks.chip`), para chips com sequências de leitura (ADCs com multiplexador: ADS1115) |

**Outras famílias da 1.0** (as chaves são validadas pelo `plan` do driver; a lista é fechada):

| Família | Chaves de endereço |
|---|---|
| `gpio` (expansores) | `line` (inteiro ou nome), `bias` (`as-is` \| `pull-up` \| `pull-down` \| `disabled`, omissão `as-is`), `active_low` (bool), `drive` (`push-pull` \| `open-drain` \| `open-source`), `edges` (`none` \| `rising` \| `falling` \| `both`, omissão `none`), `timestamp` (`kernel` \| `none`), `debounce` (duração); são as chaves por sinal do `gpio` do manifesto (ref/01 anexo A) |
| `revpi` | `var` (nome da variável do piControl) |
| `mqtt` | `topic` (string), `json` (ponteiro JSON RFC 6901, opcional), `qos` (0–1); as mesmas do manifesto (ref/01 anexo A) |
| `opcua` | `node` (NodeId, `"ns=2;s=Pump.Speed"`), `attribute` (omissão `Value`), `sampling` (duração); as mesmas do manifesto |
| `canopen` | `index` (0–0xFFFF), `subindex` (0–255), `pdo` (`"tpdo1"`…`"tpdo4"`, `"rpdo1"`…) ou SDO se omitido |
| `can` | `id` (11 ou 29 bits), `extended` (bool), `start_bit`, `length` (bits), `byte_order` |
| `mbus` | `record` (índice do registo de dados), `vif` (opcional, validação) |
| `onewire` | `field` (inteiro: o campo da leitura do sensor; o sensor é o `rom` do dispositivo, ref/01 anexo A) |
| `serial-line` | `address` (string), `field` (inteiro) |
| `x:<nome>`, `w:<nome>` | `addr` (string ou tabela opaca, passada ao `plan` do driver) |

#### 2.2.6 `quirks` por família

| Família | Chave | Tipo | Omissão | Gama |
|---|---|---|---|---|
| `modbus` | `max_regs` | inteiro | 125 | 1–125 (escrita: 1–123) |
| | `max_bits` | inteiro | 2000 | 1–2000 |
| | `max_gap` | inteiro (registos) | 8 | 0–125 |
| | `min_interval` | duração | `"0ms"` | intervalo mínimo entre pedidos ao mesmo equipamento |
| | `latency` | duração | `"10ms"` | latência de resposta do escravo; entra no orçamento do barramento (05 §5, `J0370`) |
| | `forbidden` | lista de inteiros ou `[início, fim]` | `[]` | endereços que nunca se leem nem se incluem num buraco de coalescência (`J0812`) |
| | `single_write` | `fc6` \| `fc16` | `fc6` | equipamentos que só aceitam FC 16 mesmo para um registo |
| `i2c`, `spi` | `chip` | string | — | *chip handler* do driver (`"ti.ads1115"`) |
| | `max_transfer` | inteiro (bytes) | 32 | |
| `opcua` | `max_nodes_per_read` | inteiro | 500 | |
| todas | `connect_timeout` | duração | `"5s"` | |

#### 2.2.7 Tipos crus e tipo do sinal

| `type` | Registos Modbus | Tipo do sinal por omissão |
|---|---|---|
| `bool` | coil/discrete, ou 1 bit (`bit`) | `bool` |
| `u16`, `i16` | 1 | `u64`/`i64`; `f64` se houver `scale`, `offset`, `scale_factor` ou `unit` ≠ `1` |
| `u32`, `i32` | 2 | idem |
| `u64`, `i64` | 4 | idem |
| `f32` | 2 | `f64` (ref/00 §4.1); `signal_type = "f32"` guarda `f32` |
| `f64` | 4 | `f64` |
| `bits16`, `bits32`, `bits64` | 1, 2, 4 | `bits<N>` |
| `string` | `len` registos (2 caracteres por registo) | `string` |
| `bytes` | `len` registos | `bytes` |

Um `enum` usa um tipo inteiro cru e dá `enum<Nome>`. Um NaN ou infinito num `f32`/`f64` dá `bad(nan)`. Um valor cru inteiro que não caiba no tipo do sinal (um `u64` > 2⁶³ com `signal_type = "i64"`) dá `bad(out_of_range)`.

#### 2.2.8 `[enums]` e bits

```toml
[enums]
PcsMode  = { off = 0, standby = 1, p_control = 2 }
```

- Cada enum é uma tabela `variante → inteiro`. Valores únicos e dentro da gama do tipo cru dos pontos que o usam (`J0814`).
- Um cru sem variante dá `bad(out_of_range)`.
- **Âmbito:** o enum pertence ao perfil. O seu nome completo é `<perfil>@<major>::<Nome>` (`acme/pcs-125@3::PcsState`), e é este que o registo guarda. Nas expressões do manifesto (`pcs.state == PcsState.fault`), o literal resolve-se pelo tipo do outro operando; um literal que não tenha operando tipado (`PcsState.fault` sozinho) e cujo nome exista em mais do que um perfil do site é `J0303` com a lista de candidatos. É a regra de ambiguidade desta referência, a mesma da resolução de perfis (§3, "Resolução"): uma ambiguidade é sempre um erro com os candidatos, e nunca uma escolha silenciosa.
- **Bits nomeados:** `bits = { 0 = "running", 3 = "fault" }` cria `status.running` e `status.fault` como sinais `bool` derivados, lidos com o sinal-pai (mesmo `acq_id`). Um bit fora de `0..N−1` ou dois nomes iguais são `J0814`; um nome que colida com outro sinal do perfil é `J0811`.

#### 2.2.9 `[coherent]`

```toml
[coherent]
power = ["p_total", "q_total"]
```

Tabela `nome do grupo → lista de caminhos`. Os sinais de um grupo são lidos **no mesmo pedido** (02 §6.2, 05 §4): mesma família de endereço, mesmo FC, dentro de `max_regs`, sem `forbidden` pelo meio. O buraco entre eles pode passar de `max_gap`. Um grupo que não caiba num pedido é `J0813`, detetado pelo `plan` no `jk check`. Um sinal pode estar em mais do que um grupo; os grupos que partilham sinais fundem-se. Os pares ponto/`scale_factor` são grupos implícitos.

#### 2.2.10 `[config]`: esquema da configuração do dispositivo

A chave `config = { … }` de `[devices.<d>]` no manifesto (por exemplo, num ADC I²C, `config = { pga = "4.096 V", rate = "128 SPS" }`) é validada contra esta secção.

| Chave de `[config.<chave>]` | Tipo | Obrig. | Significado |
|---|---|---|---|
| `type` | `choice` \| `bool` \| `integer` \| `number` \| `quantity` \| `string` | sim | `choice`: uma das strings de `values` (não é um literal de quantidade: `"128 SPS"` é um rótulo) |
| `values` | lista de strings | com `choice` | valores aceites |
| `unit` | unidade | com `quantity` | |
| `range` | `[mín, máx]` | não | `integer`, `number`, `quantity` |
| `default` | valor | não | sem `default`, a chave é obrigatória no manifesto |
| `apply` | lista de passos (§2.2.12) | não | escrita no `connect`, com `"$valor"` como o valor escolhido; sem `apply`, o valor vai para o `plan` do driver (*chip handlers*) |
| `description` | string | não | |

Chave de `config` não declarada, valor fora de `values` ou de `range`, ou obrigatória em falta: `J0819`. No manifesto, `config` é uma tabela nos drivers que a aceitam (ref/01 anexo A); os valores sem `apply` chegam ao driver em `device-spec.profile-config`, e os com `apply` são escritos pelo núcleo com `write-addr` (ref/06 §2.4.1, §2.10). É diferente da `config` do descritor do driver, que é o esquema das chaves diretas de `[devices.<d>]` (`address`, `unit_id`…).

#### 2.2.11 `[fallback]` (M1)

O fallback do próprio equipamento, verificado face ao `safe` e ao `safer` do manifesto (01 §8; o erro de coerência continua a ser `J0205`).

| Chave | Tipo | Obrig. | Significado |
|---|---|---|---|
| `timeout_register` | endereço de ponto (§2.2.5) com `type`, `unit` (duração) e `scale` | um de `timeout_register` ou `timeout` | registo do tempo de comunicação perdida |
| `timeout` | duração | idem | fallback com tempo fixo (não configurável) |
| `value_register` | endereço de ponto com `type`, `unit`, `scale` | um de `value_register`, `value` ou `values` | registo do valor aplicado em fallback |
| `value` | valor | idem | valor fixo |
| `values` | tabela `saída → endereço de ponto ou valor` | idem | um valor por saída (vários setpoints com um só temporizador); substitui `applies_to` |
| `applies_to` | lista de saídas do perfil | não | saídas cobertas, com `value_register`, `value` ou `behavior = "hold"`. **Sem `applies_to`** (e sem `values`), o `[fallback]` cobre **todas** as saídas do perfil (`direction = "output"`); o lock regista-as por extenso em `outputs` |
| `behavior` | `value` \| `hold` | não (omissão `value`) | `hold`: o equipamento mantém o último valor; só é coerente com `safer = "equal"` e um `safe` igual ao último aplicado, o que quase nunca é verdade: `J0205` em `prod` |
| `enable_register` | endereço de ponto + `enable_value` | não | equipamentos em que o fallback tem de ser ativado |
| `configurable` | bool | não (omissão `true` se houver registos) | `false`: o driver só lê e verifica, nunca escreve (mesmo com `fallback.configure = true` no manifesto) |

Nas entradas dos registos pode usar-se `point = "<modelo>.<ponto>"` em vez do endereço, num perfil de descoberta (§2.2.15). Referência a uma saída inexistente ou que não é `output`, ou registo sem unidade de duração: `J0820`.

**No lock.** O `jk lock --node` fixa o que leu em `[fallback.<dispositivo>]` (ref/02 §2.1.12), campo a campo: `applies_to` ou as chaves de `values` → `outputs`; `timeout_register` → `register`; `value_register` → `value_register`; o valor lido ou `timeout` → `timeout`; o valor lido ou `value` → `value`; `values` → `values`; `behavior` → `behavior`. `configured` é o `fallback.configure` do manifesto (ref/01 §5.8), sempre `false` com `configurable = false`. Os registos são lidos e escritos pelo núcleo com `read-addr`/`write-addr` (ref/06 §2.1, passos 4–5).

#### 2.2.12 `[actions]` e a linguagem de passos

```toml
[actions.reset_fault]
steps = [{ write = { at = 40300, fc = 6, value = 1 } }, { wait = "500ms" }, { write = { at = 40300, fc = 6, value = 0 } }]
```

| Chave de `[actions.<nome>]` | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `steps` | lista de passos | um de `steps` ou `native` | — | executados por ordem na fila serializada do dispositivo (02 §8) |
| `native` | tabela opaca | idem | — | operação do próprio driver (método OPC UA, SDO CANopen), validada pelo `plan` |
| `args` | tabela `nome → tipo` (`"f64 %"`, `"bool"`, `"u64"`, `"Enum"`) | não | `{}` | argumentos tipados; `"$nome"` num passo é o valor do argumento |
| `result` | tabela `nome → tipo` | não | `{}` | campos do resultado, preenchidos por passos `read` com `into` |
| `timeout` | duração | não | soma dos `wait` + 1 s por passo | por omissão da ação; o manifesto vence (17 §4.2) |
| `long` | bool | não | `false` | devolve um `ActionHandle` com progresso (M19) |
| `description` | string | não | — | |

**Passos** (cada passo é uma tabela com exatamente uma destas chaves):

| Passo | Forma | Semântica |
|---|---|---|
| `write` | `{ <endereço>, type?, value \| "$arg" }` | escreve um cru (sem escala) ou, com `signal = "<saída>"`, um valor na unidade do sinal (com escala); **não passa pelo envelope** (é uma ação, 17 §2.1), mas passa pelo permissivo e pelo papel da ação |
| `read` | `{ <endereço>, type, into = "<campo>" }` | lê e guarda no resultado |
| `wait` | duração | espera |
| `until` | `{ <endereço>, type, equals, timeout }` | lê até o valor casar ou o tempo acabar (então `Err(timeout)`) |
| `fail_if` | `{ <endereço>, type, equals, message }` | termina com `Err(message)` se o valor casar |

Endereço num `forbidden`, passo desconhecido, `wait` acima do `timeout` ou `$arg` não declarado: `J0821`. Argumentos em falta ou com tipo errado numa invocação: `J0822` (no `jk check` para ações chamadas por expressões conhecidas; em runtime, `Err(invalid_args)`).

#### 2.2.13 `[connect]`, `[relinquish]` e `[events]`

| Secção | Chaves | Significado |
|---|---|---|
| `[connect]` | `steps` (lista de passos) | executados depois de cada ligação, antes do primeiro poll (modo remoto, desbloqueio de escrita); falha → link `Down` |
| `[relinquish.<saída>]` | `steps`, ou `native = true` | o que `safe = "relinquish"` faz nessa saída (03 §6.1); `native = true` só em drivers com *priority array* (BACnet, 1.2). Uma saída com `safe = "relinquish"` sem entrada aqui e sem suporte nativo: `J0207` |
| `[events.<nome>]` | `on_rise` ou `on_change` (caminho), `fields` (tabela `campo → caminho`) | evento do dispositivo `<d>.<nome>` sem precisar de uma classe; os mesmos adaptadores de 17 §5.2 |

#### 2.2.14 `[implements]`

Um perfil (ou um modelo de simulação, §2.4.8) declara as classes que o equipamento cumpre. Cada dispositivo com o perfil é uma instância implícita dessas classes, com o nome do dispositivo (14 §5.1).

```toml
[implements."std/pump@1"]
capabilities = ["variable_speed", "hour_counter"]
map = { run_cmd = "cmd.run", running = "status.running", fault = "status.fault", speed_set = { signal = "speed_ref", scale = 0.01 }, reset = "actions.reset_fault" }
```

| Chave | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| chave da tabela | referência de classe `<pacote>/<nome>@<major>` (`std/` pode omitir-se) | — | — | classe desconhecida ou major inexistente: `J0823` |
| `capabilities` | lista | não | `[]` | capacidades que o equipamento tem; um membro obrigatório de uma capacidade declarada sem mapeamento é `J0602` |
| `map` | tabela `membro → alvo` | sim | — | membro obrigatório sem alvo: `J0601`; membro de uma capacidade **não** declarada: `J0824` |

**Alvos** (17 §5.2):

| Forma | Membros | Exemplo |
|---|---|---|
| caminho relativo de um sinal do perfil | `in`, `out`, `param` (direção compatível: `out` → `output`, `in` → `input`/`param`, `param` → `param`/`input`) | `"status.running"` |
| `"actions.<nome>"` | `action` | `"actions.reset_fault"` |
| `"const:<literal>"` | só `in` e `param` (`J0603` num `out`) | `"const:true"`, `"const:12 m3/h"` |
| `{ signal, scale?, offset?, invert? }` | `in`, `out`, `param` | `{ signal = "speed_ref", scale = 0.01 }`: membro = sinal × `scale` + `offset`; num `out`, o inverso é aplicado à proposta |
| `{ signal, enum_map }` | enums | `{ signal = "op_mode", enum_map = { off = 0, standby = 3 } }` |
| `{ edge = { rise, fall } }` | `out bool` | ações disparadas nos flancos do valor aplicado (17 §5.2) |
| `{ on_rise \| on_change, fields }` | `event` | `{ on_rise = "status.fault", fields = { code = "fault_code" } }` |

Direção incompatível, adaptador num membro de tipo errado, `scale` com dimensão (tem de ser um número) ou campo de evento inexistente: `J0824`. Sinal inexistente: `J0101`. Unidade incompatível entre o sinal e o membro: `J0107` (a conversão entre unidades da mesma dimensão é automática).

Várias instâncias implícitas por dispositivo (um ADC de 4 canais como 4 × `analog_in@1`) ficam reservadas para 1.x (chave `instances`, `J0807`); na 1.0 fazem-se como instâncias compostas no manifesto.

**Classes compostas** (documento 17, §8). Um equipamento que já é a composição inteira (um controlador Modbus que é a bomba de calor toda) cumpre a classe composta num só `[implements]`: cada membro de classe tem uma subtabela `members.<membro>`, com o `map` desse membro sobre os pontos **do mesmo perfil**.

```toml
# profiles/acme/hp-ctrl@1.profile.toml (excerto; os membros da drive@1 e da meter.ac3@1 estão abreviados)
[implements."local/heat_pump@1"]
map = { mode = { signal = "hp.mode", enum_map = { off = 0, heat = 1, boost = 2 } }, flow_sp = "hp.flow_sp" }

[implements."local/heat_pump@1".members.compressor]
map = { discharge_temp = "comp.t_discharge" }

[implements."local/heat_pump@1".members.compressor.members.drive]
map = { run_cmd = "comp.run", speed_set = { signal = "comp.speed_ref", scale = 0.1 }, speed = "comp.speed" }

[implements."local/heat_pump@1".members.t_supply]
map = { temperature = "sensors.t_flow" }

[implements."local/heat_pump@1".members.t_return]
map = { temperature = "sensors.t_return" }

[implements."local/heat_pump@1".members.meter]
map = { p_total = "power.p" }
```

| Chave | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| `members.<membro>` | tabela com `capabilities`, `map` e `members` (as chaves desta secção, recursivamente) | cond. | — | membro obrigatório sem subtabela `J0601`; membro de uma capacidade declarada sem subtabela `J0602`; `<membro>` que não é um membro de classe `J0824` |
| `members.<coleção>.<elemento>` | a mesma tabela, uma por elemento; `<elemento>` é um nome `snake_case` | cond. | — | número de elementos fora de `[min..max]` `J0653`; nome inválido `J0824` |

- Os submembros **não ganham nome de instância**: existem pelos caminhos aninhados do dispositivo (`hp1.compressor.drive.speed`, documento 16, P6). Numa coleção, o `<elemento>` faz de nome da instância do elemento no caminho (`st1.pumps.p1.running`). Um caminho aninhado igual ao de outro sinal do dispositivo é `J0604`.
- A recursão para na profundidade da classe (`JC0501`): no máximo dois níveis de `members` abaixo do `[implements]`.
- `members` é uma chave desta tabela, e não um nome de membro: os submembros de um membro de classe vão sempre em `members.<membro>.members.<submembro>`, para um membro que se chame `map` ou `capabilities` não colidir com as chaves.
- Uma subtabela liga-se só a pontos do perfil. Um membro que devia ser outro dispositivo liga-se numa instância composta no manifesto (ref/01 §5.13), e não aqui.

#### 2.2.15 `[discovery]` e o perfil `joker/sunspec@1`

```toml
[discovery]
kind   = "sunspec"
bases  = [40000, 50000, 0]
models = [1, 101, 102, 103, 120, 121, 122, 123, 160, 701, 702, 703, 704]

[discovery.names]
"103.W"            = "ac.p"
"103.Hz"           = "ac.f"
"704.WMaxLimPct"   = "w_max_lim_pct"
```

| Chave | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `kind` | `sunspec` | sim | — | único na 1.0; `canopen_lss`, `opcua_browse` reservados 1.x (`J0807`) |
| `bases` | lista de endereços | não | `[40000, 50000, 0]` | onde procurar o marcador `SunS` |
| `models` | lista de ids | não | todos os conhecidos | modelos aceites; um modelo descoberto fora da lista é ignorado com aviso |
| `max_models` | inteiro | não | 64 | |
| `names` | tabela `"<modelo>.<ponto>" → caminho` | não | — | nomes dos sinais gerados; sem nome, o caminho é `m<id>.<ponto em snake_case>`, e os grupos repetidos `m<id>.<grupo>_<n>.<ponto>` |
| `writable` | lista de `"<modelo>.<ponto>"` | não | os de acesso RW no modelo | pontos que viram `output` |

- `profile = "sunspec"` no manifesto é a forma curta de `profile = "joker/sunspec@1"`, o perfil **embutido** na runtime (lista de modelos oficiais, tabela de nomes, `implements."inverter.pv@2"`, `fallback` do modelo 704). O lock regista-o sempre pela forma completa, `[profiles."joker/sunspec@1"]` com `source = "builtin:"` (ref/01 §5.8, ref/02 §2.1.5). Um perfil de fabricante pode ter `[discovery]` e `[signals]` ao mesmo tempo (pontos fixos fora dos modelos).
- As escalas vêm dos pontos `*_SF` como `scale_factor`, lidos no mesmo lote (05 §3).
- O resultado (modelos, endereços, firmware, digest do mapa efetivo) vai para o lock (`[discovered.<d>]` com o campo `models`, que é o esquema `discovered` dos drivers Modbus; ref/02 §2.1.11, ref/06 §2.11) e só é usado em `prod` se estiver fixado (05 §7).
- Configuração inválida: `J0825`.

#### 2.2.16 Perfis gerados (Joker Link)

O driver `joker-link` gera o perfil de um MCU a partir do **catálogo** que o `HELLO` anuncia pelo hash (19 §6, ref/07 §6.3). É um perfil completo, no mesmo modelo, com `name = "joker/link-<12 hex do hash do catálogo>"`, a família `joker/link-*` (§2.2.2) e endereçamento por `SignalId` do MCU. Não se escreve, não se publica e **não entra em `[profiles]` do lock**: é fixado pelo `[discovered.<d>] digest`, que é o hash do catálogo (ref/02 §2.1.11). O `jk explain <dispositivo> --profile` mostra-o em TOML.

#### 2.2.17 Versões de um perfil

| Mudança | Classificação |
|---|---|
| endereço, FC, tipo cru, byte/word order, escala, offset, dimensão da unidade, direção, apagar ou renomear um sinal, variante de enum com outro valor, apagar um `implements` ou uma capacidade | **major** |
| sinal novo, variante nova, bit nomeado novo, ação nova, `implements` ou capacidade nova, `limits`/`stuck_detect`/`max_age`, `quirks`, `firmware`, unidade da mesma dimensão | minor |
| `label`, `description`, `docs`, hints | patch |

`jk profile diff <a> <b>` aplica a tabela; o `jk profile publish` recusa uma versão que não suba o que a tabela exige (`J0848`). Não há herança entre perfis na 1.0 (`extends` reservado, `J0807`; §5).

#### 2.2.18 Exemplo completo

```toml
# profiles/acme/pcs-125@3.profile.toml
schema = 1

[profile]
name     = "acme/pcs-125"
version  = "3.1.0"
driver   = "modbus"
vendor   = "Acme"
license  = "Apache-2.0"
firmware = ">=4.10, <5"
quirks   = { max_regs = 100, max_gap = 4, min_interval = "10ms", latency = "15ms", forbidden = [[30100, 30120]] }

[defaults]
fc = 4
word_order = "big"

[signals]
"p_ac"     = { at = 30000, type = "i32", scale = 0.001, unit = "kW", quantity = "active_power", limits = ["-150 kW", "150 kW"] }
"p_set"    = { at = 40100, fc = 16, type = "i16", scale = 0.1, unit = "kW", direction = "output", readback = "p_set_rb", safe_hint = "0 kW", range_hint = ["-125 kW", "125 kW"] }
"p_set_rb" = { at = 40100, fc = 3,  type = "i16", scale = 0.1, unit = "kW" }
"mode"     = { at = 40101, fc = 6,  type = "u16", enum = "PcsMode", direction = "output", safe_hint = "standby" }
"state"    = { at = 30010, type = "u16", enum = "PcsState" }
"alarms"   = { at = 30011, type = "bits16", bits = { 0 = "overtemp", 1 = "dc_fault", 5 = "grid_fault" } }
"serial"   = { at = 30200, type = "string", len = 8, direction = "param" }
"e_dis"    = { at = 30020, type = "u32", unit = "kWh", quantity = "energy", monotonic = true, stuck_detect = "6h" }

[enums]
PcsMode  = { off = 0, standby = 1, p_control = 2 }
PcsState = { off = 0, standby = 1, running = 2, fault = 3 }

[coherent]
power = ["p_ac", "state"]

[fallback]
timeout_register = { at = 40200, fc = 6, type = "u16", unit = "s" }
value_register   = { at = 40201, fc = 6, type = "i16", scale = 0.1, unit = "kW" }
applies_to       = ["p_set"]

[actions.reset_fault]
steps = [{ write = { at = 40300, fc = 6, value = 1 } }, { wait = "500ms" }, { write = { at = 40300, fc = 6, value = 0 } }]
timeout = "3s"

[connect]
steps = [{ write = { at = 40000, fc = 6, value = 1 } }]      # modo "controlo remoto"

[relinquish.p_set]
steps = [{ write = { at = 40000, fc = 6, value = 0 } }]      # devolve o controlo à lógica local

[events.tripped]
on_rise = "alarms.dc_fault"
fields  = { state = "state" }
```

O `[implements]` deste perfil (a classe `battery.bess@1` completa) fica de fora do exemplo, porque a classe ainda não está escrita em `.jkc`; a forma é a de §2.2.14.

#### 2.2.19 Importar um mapa de registos em CSV

A maior parte dos equipamentos Modbus vem com uma tabela de registos numa ficha técnica. `jk profile import csv` transforma essa tabela, passada a CSV, num **rascunho** de perfil: escreve tudo o que a tabela diz, e marca com `# REVER` o que ela não pode dizer. O rascunho é um perfil normal; só deixa de ser rascunho quando alguém resolve os `# REVER` e o `jk profile test` passa contra o equipamento.

```
jk profile import csv <ficheiro> --name <fabricante>/<modelo> --driver modbus [--out <ficheiro>] [--modicon]
```

| Opção | Tipo | Obrig. | Omissão | Validação e significado |
|---|---|---|---|---|
| `<ficheiro>` | caminho | sim | — | o CSV (formato abaixo); `-` lê do stdin |
| `--name` | nome de perfil (§2.1.2) | sim | — | `J0805`; vai para `[profile] name`; `local/<modelo>` para perfis do projeto que nunca se publicam |
| `--driver` | família (§2.2.2) | sim | — | na 1.0, só `modbus` (outra família: erro de utilização, código de saída 1) |
| `--out` | caminho | não | `profiles/<fabricante>/<modelo>@1.profile.toml` | o importador **nunca sobrescreve**: um ficheiro que já existe é um erro de utilização (código 1) |
| `--modicon` | — | não | desligada | lê a coluna `at` na notação antiga de base 1 com o espaço à frente: `0xxxx` coils, `1xxxx` discrete inputs, `3xxxx` input registers, `4xxxx` holding registers (5 ou 6 algarismos: `30002`, `400001`); o importador subtrai a base e confere o espaço com o `fc` (`J0810`) |

**Formato do ficheiro.** RFC 4180, UTF-8 (um BOM no início é aceite e ignorado, porque é o que as folhas de cálculo exportam). O separador é a vírgula, ou o ponto e vírgula se o cabeçalho o usar (o mesmo em todo o ficheiro). Os números usam sempre o ponto decimal: `0,1` é `J0806`, com a ajuda "usa `0.1`". Linhas começadas por `#` são comentários e linhas vazias ignoram-se. No máximo 4 096 linhas de dados (§2.1.5). A ordem das linhas é a ordem dos sinais no perfil.

**Cabeçalho obrigatório:** a primeira linha que não é comentário. Nomes em minúsculas, por qualquer ordem, cada um no máximo uma vez. `signal`, `fc` e `at` têm de estar; as outras colunas podem faltar, e uma célula vazia é a omissão da chave. Sem cabeçalho, ou sem uma das três obrigatórias, é `J0803` (o ficheiro não é um mapa de registos); uma coluna desconhecida ou repetida é `J0140`.

| Coluna | Obrig. | Célula vazia | Validação | Vai para |
|---|---|---|---|---|
| `signal` | sim | — (obrigatória em cada linha) | caminho relativo (§2.1.2, `J0805`); repetido `J0811` | a chave em `[signals]` |
| `fc` | sim | — | 1, 2, 3, 4, 5, 6, 15, 16 (`J0810`) | `fc` |
| `at` | sim | — | inteiro decimal ou `0x…`, base 0, 0–65535 (§2.2.5, `J0810`); com `--modicon`, a notação de base 1 acima | `at` |
| `type` | não | `bool` com FC 1, 2, 5, 15; `u16` com FC 3, 4, 6, 16 | tipo cru de §2.2.7 compatível com o FC (`J0809`) | `type` |
| `direction` | não | `output` com FC 5, 6, 15, 16; `input` com FC 1, 2, 3, 4 | `input` \| `output` \| `param`; `output` só com FC de escrita e `input`/`param` só com FC de leitura (`J0810`) | `direction` (omitida quando é `input`) |
| `unit` | não | `1` | catálogo de 16 §5 (`J0109`); proibida em `bool` (`J0815`) | `unit` |
| `scale` | não | `1` | número ≠ 0 (`J0806`); proibida em `bool` (`J0815`) | `scale` |
| `offset` | não | `0` | número ou quantidade (`J0806`, `J0107`) | `offset` |
| `label` | não | — | ≤ 64 B | `label` |
| `description` | não | — | ≤ 1 KiB | `description` |

Cada linha é validada com as regras do perfil (§2.2.4, §2.2.5, §2.2.7): as sobreposições de registos (`J0811`), os FC por tipo (`J0809`), as unidades (`J0109`). Os erros saem com `ficheiro:linha:coluna` do CSV, todos e não só o primeiro; com erros, não se escreve nada (código de saída 2).

**O que o importador infere, e o que deixa por rever.** O importador só escreve o que o CSV diz. O que é uma decisão sobre o equipamento, e não uma cópia da tabela, fica num comentário `# REVER: …` na linha a que diz respeito:

| Assunto | O importador | Fica `# REVER` porque |
|---|---|---|
| `[profile]` `version` | `"1.0.0"` | — |
| `vendor`, `model`, `docs`, `firmware`, `license` | não escreve | a tabela de registos não os diz; `license` é obrigatória para publicar (`J0848`) |
| `quirks` (`max_regs`, `latency`, `forbidden`, `single_write`) | não escreve | dependem do equipamento; o `jk profile test` mede a latência e sugere `latency` (§4, S9) |
| `[defaults] poll` | `"1s"` | o ritmo é uma decisão de projeto |
| `readback` de uma saída | sugere-o quando há uma entrada com o mesmo `at` num FC de leitura correspondente (FC 5 ↔ 1, FC 6/16 ↔ 3) | pode ser só coincidência de endereços |
| `safe_hint`, `range_hint` de uma saída | não escreve | é o que o equipamento aceita e o que é seguro, e nenhuma tabela diz o segundo |
| `monotonic` | sugere-o num inteiro sem sinal com unidade de contador (`s`, `h`, `Wh`, `kWh`, `MWh`, `m3`) | um tempo de funcionamento é monotónico, um temporizador não |
| `limits`, `stuck_detect`, `deadband`, `invalid` | não escreve | são do comportamento físico do equipamento |
| enums, bits nomeados, `[coherent]`, `[actions]`, `[connect]` | não escreve | o CSV não tem colunas para eles |
| `[fallback]` | não escreve; deixa um comentário no fim | se o equipamento tem fallback de comunicação, e com que tempo, está na ficha, noutra secção (§2.2.11) |
| notação dos endereços | sem `--modicon`, se todos os `at` de um FC de registos estiverem na gama `30001`–`39999` ou `40001`–`49999`, avisa | é o sinal clássico de uma ficha em notação de base 1 |

**Exemplo.** O mapa do firmware de bancada do ESP32 (documento 23, §4):

```
signal,fc,at,type,direction,unit,scale,offset,label,description
led,5,0,bool,output,,,,LED,LED da placa (GPIO2)
led_rb,1,0,bool,,,,,,
button,2,0,bool,,,,,BOOT,"botão BOOT, já invertido: 1 = premido"
touch,4,0,u16,,,,,,valor cru do sensor de toque T0 (GPIO4); desce quando se toca
rssi,4,1,i16,,,,,,RSSI do Wi-Fi em dBm; 0 sem ligação
uptime,4,2,u32,,s,,,,segundos desde o arranque
fw,3,0,u16,param,,,,,versão do firmware de bancada
```

`jk profile import csv mapa.csv --name local/bench-esp32 --driver modbus` escreve:

```toml
# profiles/local/bench-esp32@1.profile.toml
# Gerado por jk profile import csv mapa.csv. Resolve as linhas "# REVER" e corre o jk profile test.
schema = 1

[profile]
name    = "local/bench-esp32"
version = "1.0.0"
driver  = "modbus"
# REVER: vendor, model, docs, firmware (a tabela de registos não os diz)
# REVER: quirks: o jk profile test mede a latência e sugere quirks.latency

[defaults]
poll = "1s"   # REVER: ritmo de leitura

[signals]
"led"    = { at = 0, fc = 5, type = "bool", direction = "output", label = "LED", description = "LED da placa (GPIO2)" }   # REVER: readback = "led_rb"? safe_hint?
"led_rb" = { at = 0, fc = 1, type = "bool" }
"button" = { at = 0, fc = 2, type = "bool", label = "BOOT", description = "botão BOOT, já invertido: 1 = premido" }
"touch"  = { at = 0, fc = 4, type = "u16", description = "valor cru do sensor de toque T0 (GPIO4); desce quando se toca" }
"rssi"   = { at = 1, fc = 4, type = "i16", description = "RSSI do Wi-Fi em dBm; 0 sem ligação" }
"uptime" = { at = 2, fc = 4, type = "u32", unit = "s", description = "segundos desde o arranque" }   # REVER: monotonic = true?
"fw"     = { at = 0, fc = 3, type = "u16", direction = "param", description = "versão do firmware de bancada" }

# REVER: [fallback]: o equipamento tem fallback de comunicação? (ref/03 §2.2.11)
```

Resolver os `# REVER` deste rascunho dá o perfil do documento 23, §4 (`readback`, `safe_hint`, `deadband` no toque, `monotonic`, `poll = "100ms"` e o `[fallback]` de 2 s). Os comentários `# REVER` não são chaves: o `jk profile check` valida o rascunho como qualquer perfil, e lista os `# REVER` que restam como notas; o `jk profile publish` recusa um ficheiro que ainda os tenha (`J0848`).

#### 2.2.20 URIs de equipamento

A URI diz ao `jk profile test` (e ao `jk discover`, documento 05 §7) como chegar a um equipamento **sem manifesto**: protocolo, porta ou endereço, e unidade. Vai tal e qual para o campo `uri` de `POST /v1/profile-tests` e `POST /v1/discovery` (ref/09 §5.17). Uma URI mal formada, com um esquema desconhecido ou com um parâmetro desconhecido é `J-E1708`, no portátil e no nó.

| Esquema | Forma | Parâmetros (`?a=b&c=d`) | Omissões | Driver |
|---|---|---|---|---|
| `modbus-rtu` | `modbus-rtu:<porta>?baud=<b>&unit=<u>` | `baud` (300–4 000 000, **obrigatório**), `unit` (1–247), `parity` (`none` \| `even` \| `odd`), `stop` (1 \| 2), `data_bits` (7 \| 8) | `unit=1`, e as omissões de um barramento `serial` (ref/01 §5.6): `parity=none`, `stop=1`, `data_bits=8` | `modbus-rtu` |
| `modbus-tcp` | `modbus-tcp://<host>[:<porta>]?unit=<u>` | `unit` (0–255) | porta 502, `unit=1` | `modbus-tcp` |
| `sunspec` | `sunspec://<host>[:<porta>]?unit=<u>` | `unit` (0–255) | porta 502, `unit=1` | `modbus-tcp`, com o perfil `joker/sunspec@1` e a descoberta (§2.2.15) |
| `i2c` | `i2c:<barramento>?address=<a>` | `address` (0x03–0x77, **obrigatório**, decimal ou `0x…`) | — | `i2c` |

- **`<porta>`** é um caminho absoluto de dispositivo (`/dev/serial/by-id/usb-Silicon_Labs_CP2102_USB_to_UART_Bridge_Controller_0001-if00-port0`) ou, com `--node`, o nome de uma porta do `[serial]` da placa do nó (`modbus-rtu:uart0?baud=9600`, §2.3.2). É tudo o que está entre `modbus-rtu:` e o `?`; os caracteres `?`, `&`, `#`, `%` e o espaço escrevem-se `%XX`. Uma porta que não existe é `J0828`. **Escreve-se sempre o caminho `by-id`**: o `/dev/ttyUSB0` muda de número quando se liga outro adaptador.
- **`<barramento>`** do `i2c`: caminho (`/dev/i2c-1`) ou nome do `[i2c]` da placa (`i2c1`).
- **`<host>`** é um nome DNS, um IPv4, ou um IPv6 entre `[ ]`. Só no `jk discover`, `<host>` pode ser uma rede CIDR (`modbus-tcp://192.168.10.0/24`) e a unidade uma gama (`--unit 1..10`, ou `?unit=1..10`); no `jk profile test`, um só equipamento e uma só unidade.
- **Nunca há credenciais numa URI.** Uma URI com `<utilizador>@` ou com um parâmetro `password` é `J-E1708`. Os equipamentos que precisam de credenciais ou de segurança de transporte (`opcua`, `mqtt`) testam-se com um manifesto e o `jk check --node`, onde as credenciais são `secret:` (ref/01 §3.10); é por isso que não têm esquema de URI na 1.0.
- A URI não substitui o perfil: diz só **onde** está o equipamento. As chaves do perfil (`quirks`, endereços) aplicam-se como num dispositivo do manifesto.

---

### 2.3 Placas

Um perfil de placa descreve o hardware fixo de um **modelo** de computador (Linux) ou de microcontrolador (Joker Micro). É referido por `[target] board = "<nome>"` (ref/01) e por `jk build --target <nome>`.

#### 2.3.1 `[board]`

| Chave | Tipo | Obrig. | Omissão | Validação e significado |
|---|---|---|---|---|
| `name` | string | sim | — | §2.1.2, igual ao nome do ficheiro (`J0804`) |
| `version` | SemVer | sim | — | versões como os perfis (§2.3.6) |
| `os` | `linux` \| `none` | não | `linux` | `none` = Joker Micro |
| `arch` | `aarch64` \| `x86_64` \| `armv7` \| `riscv64` \| `armv6` (Linux; `armv6` só com `tier = "dev"`, documento 23); triplo Rust (MCU: `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `xtensa-esp32s3-none-elf`, `xtensa-esp32-none-elf`) | sim | — | incoerente com `os`: `J0829` |
| `tier` | `"1"` \| `"2"` \| `"dev"` (string) | não | `"2"` | nível de suporte (01 §22) |
| `mcu_class` | `m1` \| `m2` | com `os = "none"` | — | 19 §3.1; num Linux: `J0829` |
| `chip` | string | com `os = "none"` | — | o nome do chip **como o HAL o escreve** (`"rp235xa"`, `"stm32g474re"`, `"esp32"`, `"esp32c6"`), que o `jk build` passa ao HAL como feature; decide a família `chip-*` do `joker-micro` (ref/08 §2.11.2, §2.15.1). Um chip que nenhum HAL suportado conhece, ou incoerente com `arch`, é `J0829`; num Linux, `J0829`. É distinto de `[probe] chip` (§2.3.3), que é o nome do `probe-rs` |
| `vendor`, `model`, `description`, `docs`, `license`, `authors` | strings | não | — | como em `[profile]` |
| `cpus` | inteiro | sim em Linux | — | núcleos |
| `memory` | quantidade IEC (`"8 GiB"`) | sim | — | RAM; base do `J0380` (70%) |
| `flash` | quantidade IEC | com `os = "none"` | — | |
| `storage` | `"sd"` \| `"emmc"` \| `"ssd"` | não (só Linux) | `"sd"` (o pior caso) | o meio onde vive `/var/lib/joker` (historian, WAL, registo de decisões, `kv`, traço). O `jk check` projeta a carga de escrita do manifesto (bytes por dia e fsyncs por hora) e compara-a com o limiar do meio; o `jk doctor` mede-a no nó. Acima do limiar, o aviso `J0381` com as fontes, por ordem (documento 01, §12 e §19). O `jk board probe` propõe-na a partir do dispositivo da raiz (`/sys/block/<dev>/device/type`: `SD` → `sd`, `MMC` → `emmc`; NVMe ou SATA → `ssd`). Num MCU (`os = "none"`): `J0829` |
| `fpu` | `none` \| `sp` \| `sp+dcp` \| `dp` | não | `dp` em Linux, `none` em MCU | o mesmo enum do descritor de firmware (ref/08 §2.14, chave 15; `fpu` no `spec/cddl/joker-micro.cddl`): `none` = sem FPU, `f32` e `f64` por software (`J0780` para `f32`, `J0721` para `f64`); `sp` = precisão simples em hardware, `f64` por software (`J0721`); `sp+dcp` = precisão simples mais um coprocessador de dupla precisão, como o DCP do RP2350 (o `f64` é acelerado e não dá `J0721`); `dp` = simples e dupla em hardware |
| `reset_safe` | bool | com `os = "none"` | — | `true` se as saídas ficam num estado seguro no reset do hardware, antes de o firmware configurar os pinos (ref/08 §2.12.3); `false` é `J0786` em `prod`, aceitável com justificação por `[target] accept_unsafe_reset` no manifesto do MCU (ref/01 §5.22) |
| `stack` | quantidade IEC | com `os = "none"` | — | pilha disponível para o nó (soma dos níveis de interrupção ativos); o `jk check` compara-a com as pilhas medidas no build (`J0774`, ref/08 §2.9.3) |
| `clock` | quantidade (`"150 MHz"`) | não | — | frequência nominal |
| `recommended` | tabela `classe → lista de CPUs` | não | — | sugestão para `[resources]` (01 §2); CPUs fora de `0..cpus−1`: `J0829`. Com `cpus = 1`, todas as classes partilham a CPU 0 (`{ rt = [0], io = [0], bg = [0] }`), e essa sobreposição com `rt` é uma **nota** do `jk check`, não o `J0360` (ref/01 §5.4) |
| `kernel` | `preempt_rt` \| `standard` | não | `standard` | kernel com que a placa é suportada; o `jk doctor` compara |
| `boot_cmdline` | lista de strings | não | `[]` | sugestões (`isolcpus=3`, `nohz_full=3`) mostradas pelo `jk doctor --rt` |

#### 2.3.2 Periféricos em Linux

| Secção | Forma | Significado |
|---|---|---|
| `[watchdog]` | `device` (caminho), `timeout` (duração), `min_timeout`, `max_timeout` | watchdog de hardware (01 §8) |
| `[gpio]` | `chip` (label ou caminho: `"pinctrl-rp1"`), `lines` (tabela `nome → offset`, opcional; sem ela, usam-se os nomes do kernel) | resolve `chip = "auto"` e `line = "GPIO17"` do manifesto; linha desconhecida: `J0828` |
| `[io.<nome>]` | `driver`, endereço da família (`line`, `var`…), `direction`, `fallback` (valor) | IO fixo da placa (LEDs, relés), com o caminho `board.<nome>` (prefixo reservado, ref/00 §2); `fallback` é o estado que o **hardware** impõe quando o `jokerd` ou a placa param (M1): conta como fallback do dispositivo para esse sinal, e o `J0205` compara-o com o `safe` do manifesto como qualquer outro. Não há `safe` nem `safe_hint` numa placa (ADR 0006; A6) |
| `[serial]` | tabela `nome → caminho` ou `{ path, rs485 = true, max_baud }` | portas série estáveis |
| `[i2c]`, `[spi]`, `[can]` | tabela `nome → caminho` ou interface | barramentos da placa |
| `[net]` | tabela `nome → interface` | interfaces com nome estável |
| `[accel.<nome>]` | `kind`, `device` | aceleradores garantidos (`"npu:hailo0"`); **reservado para 1.1** mas aceite já (Preparado para ML) |

Um nome de periférico referido pelo manifesto que não exista na placa é `J0828`.

**O IO fixo só existe quando é referido.** Um `[io.<nome>]` da placa não abre nada, não cria sinais nem ocupa recursos enquanto o manifesto não referir `board.<nome>` (numa ligação, num `[outputs."board.<nome>"]`, numa instância ou num seletor que o apanhe explicitamente; o seletor `**` não conta). Referido, o `jokerd` abre-o como um dispositivo implícito com o driver e o endereço da placa. Daí duas regras:

- **Um recurso do kernel, um dono.** Um dispositivo do manifesto que use o mesmo recurso que um `board.<nome>` referido (o mesmo LED de `/sys/class/leds`, a mesma linha GPIO do mesmo `chip`, a mesma zona térmica) é `J0178` (endereço duplicado, ref/01 §5.8), como dois dispositivos no mesmo endereço de um barramento. Se o `board.<nome>` não for referido, o dispositivo do manifesto pode usar o recurso, e o IO fixo não existe nessa geração.
- **O `fallback` do `[io]` só conta pelo caminho `board.<nome>`.** É um facto do hardware desse IO, e o `J0205` só o compara com o `safe` de `[outputs."board.<nome>"]`. Um dispositivo do manifesto que use o mesmo recurso por outro caminho não herda o `fallback`: sem fallback próprio, é uma saída sem fallback de dispositivo (e o `J0205` di-lo em `prod`).

#### 2.3.3 Microcontroladores (`os = "none"`)

| Secção | Chaves | Significado |
|---|---|---|
| `[capacities]` | os nomes da tabela de capacidades da ref/08 §2.13.3 (`signals`, `outputs`, `groups`, `instances`, `apps`, `alarms`, `programs`, `trace_bytes`, `kv_bytes` ≤ 16 KiB, …), inteiros ou, nos `_bytes`, quantidades IEC; nome desconhecido `J0140` | capacidades **por omissão** do firmware genérico desta placa (19 §4.1): são as que o `J0710` usa, as que o lock fixa em `[mcu.<d>] capacity` (ref/02 §2.1.10) e as que `[target.capacity]` do manifesto sobrepõe (ref/01 §5.22); a memória estimada acima de `memory`/`flash` é `J0830` |
| `[irq]` | `priorities` (inteiro: níveis de prioridade de interrupção utilizáveis pelos grupos), `reserved` (lista: níveis que o firmware não pode usar, p. ex. os do rádio) | executores de interrupção dos grupos, por ordem rate-monotonic (ref/08 §2.12.3, passo 7); mais grupos do que níveis é `J0829` |
| `[areas]` | `bootloader`, `boot_state`, `firmware`, `fw_staging`, `gen_a`, `gen_b`, `persist`, `identity`, cada uma `{ offset, size }` | partições da flash (ref/08 §2.12.2); sobreposição ou fora de `flash` é `J0829`; `firmware` e `fw_staging` do mesmo tamanho |
| `[pins.<nome>]` | `functions` (lista: `gpio`, `pwm`, `adc`, `touch`, `uart`, `i2c`, `spi`, `can`), `adc_channel`, `touch_channel` (inteiro: o pad de toque capacitivo ligado a este pino, `T0` = 0; só com a função `touch`), `note` | pinos físicos; nomes como impressos na placa (`"gp15"`, `"pa5"`, `"gpio4"`). O `touch_channel` é o que o driver `esp32` resolve em `touch = <n>` (ref/01 anexo A); dois pinos com o mesmo canal é `J0829` |
| `[uart.<nome>]`, `[i2c.<nome>]`, `[spi.<nome>]`, `[can.<nome>]` | pinos (`tx`, `rx`, `de`; `sda`, `scl`; `sck`, `mosi`, `miso`, `cs`; `tx`, `rx`), `max_baud`/`max_hz` | periféricos e os pinos que ocupam; um pino em dois periféricos ativos é `J0829` |
| `[adc]` | `bits`, `vref` (quantidade), `max_rate` | |
| `[watchdog]` | `kind` (`"iwdg"`, `"rp2350"`), `timeout` | watchdog de hardware; o firmware arma-o no passo 1 do arranque (ref/08 §2.12.3) |
| `[link]` | `transports` (lista: `uart`, `rs485`, `can`, `usb_cdc`, `udp`, `tcp`), `default` (`{ transport, port, baud, addr }`: a ligação do **modo de manutenção**, sem geração válida) | transportes do Joker Link possíveis nesta placa (19 §6); `default` é o link por omissão (ref/08 §2.12.3) |
| `[boot]` | `bootloader` (`embassy-boot` \| `mcuboot` \| `esp-idf`, o bootloader de segundo estágio da Espressif com partições OTA), `slots` (2) | bootloader e troca A/B; as partições estão em `[areas]` |
| `[probe]` | `tool` (`probe-rs` \| `espflash`, omissão `probe-rs`), `chip` (nome do probe-rs; só com `probe-rs`), `protocol` (`swd` \| `jtag` com `probe-rs`; `serial` com `espflash`) | `jk flash`: por sonda, ou por série sem sonda (ESP32, documento 23 §7) |
| `[net]` | `wifi` (bool), `ethernet` (bool) | só `m2` |

#### 2.3.4 Exemplo Linux

```toml
# boards/rpi5.board.toml
schema = 1

[board]
name    = "rpi5"
version = "1.0.0"
arch    = "aarch64"
tier    = "1"
cpus    = 4
memory  = "8 GiB"
storage = "ssd"
kernel  = "preempt_rt"
recommended  = { rt = [3], io = [2], ml = [1, 2], bg = [0] }
boot_cmdline = ["isolcpus=3", "nohz_full=3", "rcu_nocbs=3"]

[watchdog]
device  = "/dev/watchdog0"
timeout = "15s"

[gpio]
chip = "pinctrl-rp1"

[i2c]
"i2c1" = "/dev/i2c-1"

[serial]
"uart0" = "/dev/ttyAMA0"
```

#### 2.3.5 Exemplo MCU

```toml
# boards/rp2350-pico2.board.toml
schema = 1

[board]
name      = "rp2350-pico2"
version   = "1.0.0"
os        = "none"
arch      = "thumbv8m.main-none-eabihf"
mcu_class = "m1"
chip      = "rp235xa"
tier      = "1"
memory    = "520 KiB"
flash     = "4 MiB"
fpu       = "sp+dcp"
clock     = "150 MHz"

reset_safe = true
stack     = "16 KiB"

[capacities]
signals     = 256
outputs     = 64
groups      = 4
instances   = 32
apps        = 2
alarms      = 32
programs    = 64
trace_bytes = "16 KiB"
kv_bytes    = "16 KiB"

[irq]
priorities = 4

[pins.gp0]
functions = ["gpio", "uart"]
[pins.gp1]
functions = ["gpio", "uart"]
[pins.gp26]
functions = ["gpio", "adc"]
adc_channel = 0

[uart.uart0]
tx = "gp0"
rx = "gp1"
max_baud = 3000000

[adc]
bits = 12
vref = "3.3 V"

[watchdog]
kind    = "rp2350"
timeout = "500ms"

[link]
transports = ["uart", "rs485", "usb_cdc"]
default    = { transport = "usb_cdc" }

[boot]
bootloader = "embassy-boot"
slots      = 2

[areas]
bootloader = { offset = 0x000000, size = 0x008000 }
boot_state = { offset = 0x008000, size = 0x002000 }
firmware   = { offset = 0x010000, size = 0x100000 }
fw_staging = { offset = 0x110000, size = 0x100000 }
gen_a      = { offset = 0x210000, size = 0x010000 }
gen_b      = { offset = 0x220000, size = 0x010000 }
persist    = { offset = 0x230000, size = 0x010000 }
identity   = { offset = 0x240000, size = 0x002000 }

[probe]
chip = "RP235x"
protocol = "swd"
```

#### 2.3.6 Versões de uma placa

Mesma política dos perfis: mudar um caminho de periférico, um pino ou uma partição é **major**; acrescentar periféricos ou pinos é minor; texto é patch. O manifesto refere a placa pelo nome (`board = "rpi5"`, que quer dizer a maior major conhecida) ou com major (`board = "rpi5@1"`); o lock fixa versão e digest (ref/02).

#### 2.3.7 Escrever uma placa nova

Quando o `[target] board` de um manifesto não existe no projeto nem no registo (`J0172`, ref/01), escreve-se o perfil da placa. Há três pontos de partida, e o fim é sempre o mesmo ficheiro `boards/<placa>.board.toml`, validado pelo `jk board check`:

| Comando | Parte de | Serve quando |
|---|---|---|
| `jk board probe --node <nó> [--name <placa>] [--out <ficheiro>]` | o inventário do nó real (`GET /v1/node/hardware`, ref/09 §5.2) | a placa já tem o `jokerd` instalado e enrolado (é o caso normal) |
| `jk board new <nome> --from <placa>` | um perfil existente, do projeto ou do registo | a placa é uma variante de outra (mais memória, outro módulo de rádio) |
| escrever à mão | a ficha técnica | um MCU (`os = "none"`): as áreas da flash, as prioridades de interrupção e as capacidades não se leem de um nó |

As três escrevem `version = "1.0.0"`, e **nunca sobrescrevem** um ficheiro que já existe (erro de utilização, código de saída 1).

**O que o `jk board probe` infere, e o que é decisão humana.** O inventário diz o que existe **agora** naquele nó. O perfil diz o que o **modelo** garante, e o que o Joker deve esperar dele. O probe escreve o que o inventário prova, e marca com um comentário `# REVER: …` o que só uma pessoa pode decidir:

| Chave | O probe escreve | A partir de | `# REVER` |
|---|---|---|---|
| `name` | `--name`, ou o modelo em minúsculas com hífens (`raspberry-pi-zero-w-rev-1-1`) | `model` | só se foi derivado do modelo |
| `arch` | o valor normalizado | `arch` | — |
| `model` | o modelo, tal e qual | `model` | — |
| `vendor`, `docs`, `license` | nada | — | sim: o inventário não os sabe |
| `cpus` | as CPUs possíveis | `cpus.possible` | — |
| `memory` | a potência de 2 em MiB acima do `MemTotal` (o `MemTotal` desconta a memória do GPU e do kernel: 427 MiB → `"512 MiB"`) | `memory.total` | — |
| `clock` | a frequência máxima | `cpus.max_freq` | — |
| `storage` | o tipo do dispositivo da raiz (`sd`, `emmc`, `ssd`) | `storage` | — |
| `tier` | `"dev"` | — | **sim**: o nível é um compromisso de suporte (01 §22), e `"1"` ou `"2"` exige CI com a placa |
| `kernel` | `"preempt_rt"` se o kernel atual o é, senão `"standard"` | `kernel.preempt_rt` | **sim**: é o kernel **alvo** com que a placa é suportada, e não o que está instalado hoje |
| `recommended` | uma sugestão: com 1 CPU, tudo em `[0]`; com 2, `rt = [1]`, `bg = [0]`, `io = [0]`; com 3 ou mais, `rt` = a última, `io` = a penúltima, `bg` = as restantes | `cpus.count` | **sim**: depende do que corre na placa além do Joker |
| `boot_cmdline` | `isolcpus`, `nohz_full` e `rcu_nocbs` das CPUs `rt` sugeridas (nada com 1 CPU) | `recommended` | **sim**, com as linhas de arranque atuais num comentário |
| `[watchdog]` | `device` do primeiro watchdog; `timeout` = o menor de 15 s e `max_timeout`; `min_timeout`, `max_timeout` | `watchdogs` | — |
| `[gpio] chip` | o `label` do chip do SoC (`pinctrl-*`, ou o de mais linhas); os outros num comentário, com as linhas ocupadas | `gpio_chips` | — |
| `[io.<nome>_led]` | um por LED: `driver = "sysfs"`, `led`, `direction = "output"` (`ACT` → `[io.act_led]`) | `leds` | **sim, o `fallback`**: o que o LED faz sozinho quando o `jokerd` ou a placa param. O probe não o pode saber; escreve o `trigger` atual e deixa a linha `fallback` comentada. Sem `fallback`, a saída não tem fallback de dispositivo, e o `J0205` diz-o em `prod` (§2.3.2) |
| `[io.cpu_temp]`, `[io.<tipo>_temp]` | um por zona térmica: `driver = "sysfs"`, `thermal` (a zona `cpu-thermal` chama-se `cpu_temp`) | `thermal_zones` | — |
| `[serial]` | só as UARTs da placa (`soc = true`), pelo nome estável (`"uart0" = "/dev/serial0"`) | `serial` | os adaptadores USB-série **não** entram: não são da placa. Vão num comentário, com o caminho `by-id`, para copiar para um `[buses]` do manifesto |
| `[i2c]`, `[spi]` | os que existem (`"i2c1" = "/dev/i2c-1"`; `"spi0"` para `spidev0.0`, `"spi0_1"` para `spidev0.1`) | `i2c`, `spi` | se não houver nenhum, um comentário diz que podem estar desligados na configuração de arranque |
| `[net]` | `"eth"` e `"wifi"` (depois `"eth1"`…) para as interfaces físicas; nunca `loopback` nem `virtual` | `net` | — |
| TPM, USB | só comentários | `tpm`, `usb` | — (a placa não declara TPM; o `jk doctor` verifica-o, `J0933`) |

O probe nunca escreve `[gpio] lines` (usam-se os nomes do kernel), `[accel]` nem `[can]`. Se o nó já declara uma placa conhecida (`GET /v1/info`), o probe di-lo antes de escrever: talvez não seja preciso uma placa nova.

**Exemplo: o Raspberry Pi Zero W** (a placa da trilha, parte 6). `jk board probe --node zero --name rpi-zero-w` escreve:

```toml
# boards/rpi-zero-w.board.toml
# Gerado por jk board probe --node zero. Resolve as linhas "# REVER" e corre o jk board check --node zero.
schema = 1

[board]
name    = "rpi-zero-w"
version = "1.0.0"
arch    = "armv6"
model   = "Raspberry Pi Zero W Rev 1.1"
# REVER: vendor, docs, license
tier    = "dev"                          # REVER: nível de suporte (01 §22); "1" ou "2" exige CI com esta placa
cpus    = 1
memory  = "512 MiB"                      # MemTotal 427 MiB
clock   = "1 GHz"
storage = "sd"                           # mmcblk0, tipo SD
kernel  = "standard"                     # REVER: kernel alvo; o atual é 6.12.25+rpt-rpi-v6, sem PREEMPT_RT
recommended = { rt = [0], io = [0], bg = [0] }   # REVER: com 1 CPU não há isolamento
# linhas de arranque atuais: console=serial0,115200 console=tty1 root=PARTUUID=… rootwait

[watchdog]
device      = "/dev/watchdog0"           # bcm2835-wdt
timeout     = "15s"
min_timeout = "1s"
max_timeout = "15s"

[gpio]
chip = "pinctrl-bcm2835"                 # 54 linhas; ocupadas pelo kernel: 47 (ACT)

[io.act_led]
driver    = "sysfs"
led       = "ACT"
direction = "output"
# fallback = false                       # REVER: o que o LED faz quando o jokerd para (trigger atual: actpwr)

[io.cpu_temp]
driver  = "sysfs"
thermal = "thermal_zone0"                # cpu-thermal, 41.2 °C agora

# [serial], [i2c], [spi]: nenhum no nó (podem estar desligados em /boot/firmware/config.txt)

[net]
"wifi" = "wlan0"

# TPM: não há. USB: nada ligado.
```

**`jk board check`.** Valida o ficheiro com as regras desta secção (`J0800`–`J0849`: esquema, nomes, `J0829` para incoerências internas). Com `--node`, lê o inventário do nó e confirma, só com leituras:

| Verificação | Contra | Falha |
|---|---|---|
| `arch` e `cpus` | `arch`, `cpus.possible` | `J0829` |
| `memory` ≥ `MemTotal` e < 2 × `MemTotal` | `memory.total` | `J0829` |
| `[watchdog] device` existe; `timeout` entre `min_timeout` e `max_timeout` do nó | `watchdogs` | `J0828`; `J0829` |
| `[gpio] chip` existe; cada `lines` existe; nenhuma linha de um `[io.*]` `gpio` está ocupada pelo kernel | `gpio_chips` | `J0828`, com o `consumer` que a ocupa |
| cada `[io.*]` `sysfs` existe (`led`, `thermal`) | `leds`, `thermal_zones` | `J0828` |
| cada caminho de `[serial]`, `[i2c]`, `[spi]` existe | `serial` (incluindo `aliases`), `i2c`, `spi` | `J0828` |
| cada interface de `[net]` existe | `net` | `J0828` |
| `kernel`, `boot_cmdline` | `kernel`, `cmdline` | não falha: é o `jk doctor` que compara o kernel alvo com o instalado; o `check` mostra a diferença como nota |

Os `# REVER` que restam aparecem no fim como notas, com a linha. Um exemplo, depois de se ter acrescentado à mão um `[i2c]` que o nó não tem:

```
$ jk board check boards/rpi-zero-w.board.toml --node zero
→ zero (dev) · de --node
error[J0828]: recurso inexistente na placa: `/dev/i2c-1`
  --> boards/rpi-zero-w.board.toml:38:10
   |
38 | "i2c1" = "/dev/i2c-1"
   |          ^^^^^^^^^^^^ o nó zero não tem este dispositivo
   = ajuda: o I²C pode estar desligado (dtparam=i2c_arm=on em /boot/firmware/config.txt); se a placa não o garante, tira a linha
nota: 2 linhas "# REVER" por resolver (8, 17)
```

**Quando publicar.** Uma placa em `boards/` do projeto resolve antes do registo (§3, "Resolução") e entra no lock pelo digest do ficheiro, como uma publicada: para um só projeto, **não é preciso publicar**, e uma placa `tier = "dev"` normalmente fica assim (é o caso da `rpi-zero-w` da trilha). Uma placa `tier = "dev"` **nunca se publica** (`J0848`, §2.8.4): partilha-se como ficheiro (`boards/` de cada projeto, ou um repositório Git comum trazido para dentro do projeto, como submódulo). Publica-se (`jk board publish`, §2.8.4) quando outros projetos a vão usar, com `tier = "1"` ou `"2"`, e só depois de: todos os `# REVER` resolvidos; `license` escrita; `jk board check --node` a passar em pelo menos uma unidade real; e o `jk doctor` do nó sem erros com o `kernel` declarado. Publicar uma placa com `tier = "1"` ou `"2"` é também um compromisso de CI (documento 18).

---

### 2.4 Modelos de simulação

Um modelo de simulação é um **dispositivo simulado** servido pelo driver `sim` (04 §6): `[devices.<d>] driver = "sim"`, `model = "sim/<modelo>.sim.toml"` (ref/01). Os seus sinais aparecem como `<d>.<sinal>`, exatamente como os de um dispositivo real.

#### 2.4.1 Estrutura

```toml
schema = 1

[model]        # metadados e passo de integração (opcional)
[params]       # parâmetros do modelo (opcional)
[signals]      # sinais
[dynamics]     # equações
[faults]       # falhas programadas e teclas de falha
[enums]        # como nos perfis
[actions]      # ações simuladas
[implements]   # como nos perfis
[plant]        # planta em Wasm (em vez de [dynamics])
```

#### 2.4.2 `[model]`

| Chave | Tipo | Omissão | Significado |
|---|---|---|---|
| `description` | string | — | |
| `step` | duração | `"10ms"` | passo fixo de integração e de amostragem, em tempo virtual (e em tempo real no `jk dev`) |
| `latency` | duração | `"0ms"` | atraso entre o valor da planta e a entrega da amostra ao núcleo (latência por pedido, 04 §6) |
| `seed` | inteiro u64 | derivado do caminho do dispositivo (FNV-1a de 64 bits) | semente do ruído e do ressalto; o cenário pode sobrepô-la |
| `profile` | referência de perfil (`"<nome>@<major>"`, como em `[devices.<d>] profile`) | — | o modelo **reutiliza os sinais do perfil** (abaixo) |

**`profile`: um modelo com os sinais de um perfil.** Com `[model] profile = "<ref>"`, o perfil resolve-se como qualquer outro (§3, "Resolução") e fica no lock em `[profiles]` (ref/02 §2.1.5). O modelo passa a ter, com os mesmos nomes, os `[signals]` do perfil com `type`, `enum`, `unit`, `quantity`, `direction` e `limits` do perfil, e herda os seus `[enums]` e `[implements]`. O resto do perfil (endereços, `quirks`, `[config]`, `[fallback]`, `[actions]`, `[connect]`, `[discovery]`) não se aplica a um dispositivo `sim`. No `[signals]` do modelo:

- uma entrada com o nome de um sinal do perfil só acrescenta as chaves da simulação (`init`, `gen`, `noise`, `resolution`, `bounce`, `latency`, `panel`, `label`, `description`); repetir `type`, `unit`, `direction` ou `enum` com outro valor é `J0815`;
- uma entrada com um nome que o perfil não tem é um sinal só da simulação, declarado por inteiro como sem `profile`;
- um sinal do perfil sem entrada no modelo é constante no seu `init` (a omissão da tabela abaixo).

É o que permite trocar `driver = "modbus-tcp", profile = "<ref>"` por `driver = "sim", model = "…"` num overlay sem mudar os tipos dos sinais nem as instâncias (documento 23, §6.5). Uma referência que não resolve para um perfil é `J0803`.

#### 2.4.3 `[params]`

Parâmetros que o manifesto dá ao modelo (`[devices.<d>] params = { … }`, ou `sim = { … }` numa instância de classe, ref/01).

```toml
[params]
mean   = { default = "22 °C" }
amp    = { default = "3 K" }
period = { default = "10m" }
```

| Chave de `[params.<nome>]` | Tipo | Obrig. | Significado |
|---|---|---|---|
| `default` | valor | não | sem `default`, o parâmetro é obrigatório |
| `unit` | unidade | não | inferida do `default` |
| `description` | string | não | |

Em qualquer valor de `init`, `gen`, `noise` e `[faults]`, a string `"$<nome>"` é substituída pelo parâmetro. Nas expressões de `[dynamics]`, o parâmetro é `param.<nome>`. Parâmetro desconhecido, sem valor ou com dimensão errada: `J0836`.

#### 2.4.4 `[signals]`

| Chave | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `type` | tipo do sinal (02 §2) | sim | — | `enum<Nome>` escreve-se `enum = "Nome"` com `type` omitido |
| `enum` | nome em `[enums]` | não | — | |
| `unit`, `quantity` | como nos perfis | não | `1` | |
| `direction` | `input` \| `output` \| `param` | não | `input` | uma `output` é escrita pelo núcleo; o seu valor é o **aplicado** entregue ao driver `sim` |
| `init` | valor | não | `false`, `0` ou a primeira variante | estado inicial; obrigatório em sinais com `integrate`, `lag` ou `delay` (`J0833`) |
| `gen` | gerador (§2.4.5) | não | — | fonte do valor; exclusivo com `[dynamics]` (`J0832`) |
| `noise` | quantidade | não | — | ruído gaussiano aditivo (desvio padrão) sobre o valor gerado ou calculado |
| `resolution` | quantidade | não | — | quantização (ADC) |
| `bounce` | duração | não | — | só em `bool`: cada mudança é entregue com ressalto durante este tempo, como flancos com carimbo exato (independente de `step`) |
| `latency` | duração | `[model].latency` | por sinal |
| `limits` | `[mín, máx]` | não | — | limites físicos, como num perfil (a planta pode passar deles; a aquisição marca `bad(out_of_range)`) |
| `panel` | tabela (§2.4.7) | não | — | presença no painel do `jk dev --panel` |
| `label`, `description` | strings | não | — | |

Um sinal `input` sem `gen`, sem `[dynamics]` e sem `panel` é constante no `init`. Uma `output` com `gen` ou `[dynamics]` é `J0832`.

#### 2.4.5 Geradores

`gen = { kind = …, … }`. Os valores numéricos nus estão na unidade do sinal; os literais com unidade convertem-se.

| `kind` | Chaves (obrig. em negrito) | Valor em `t` |
|---|---|---|
| `constant` | **`value`** | `value` |
| `step` | **`from`**, **`to`**, **`at`** | `from` antes de `at`, `to` a partir de `at` |
| `ramp` | **`from`**, **`to`**, **`over`**, `at` (omissão `"0s"`) | linear de `from` a `to` entre `at` e `at + over`, constante depois |
| `sine` | **`mean`**, **`amp`**, **`period`**, `phase` (graus, omissão 0) | `mean + amp · sin(2π t/period + phase)` |
| `square` | **`low`**, **`high`**, **`period`**, `duty` (0–1, omissão 0.5) | |
| `noise` | **`mean`**, **`std`** | gaussiano, semente do modelo |
| `walk` | **`start`**, **`std`** (por `step`), `min`, `max` | passeio aleatório limitado |
| `replay` | **`file`** (CSV ou Parquet), **`column`**, `time_column` (omissão `t`), `loop` (bool) | valores gravados, interpolação de degrau |

`amp`, `std` e diferenças usam a unidade de **diferença** quando o sinal está em `°C` (`amp = "3 K"`, 16 §4). Gerador desconhecido ou chave em falta: `J0831`.

#### 2.4.6 `[dynamics]`

```toml
[dynamics]
level = "integrate((inflow - 0.05 m3/h/% * pump) / 3.2 m2, min = 0 m, max = 3 m)"
```

Cada entrada `sinal = "expressão"` define o valor de um sinal `input` a cada `step`. A linguagem é a de 02 §10 com três extensões, válidas **só** aqui (fora de `[dynamics]`: `J0833`):

```
call     := nome "(" arg ("," arg)* ")"
arg      := expr | nome "=" expr               (argumentos nomeados: só nas funções abaixo)
```

| Função | Dimensão | Semântica determinista |
|---|---|---|
| `integrate(d, min = a, max = b)` | `unidade(d) · s` = unidade do sinal | Euler explícito: `x ← clamp(x + d · step, a, b)`; `min`/`max` opcionais |
| `lag(u, tau)` | a de `u` | `x ← x + (u − x) · step / tau` |
| `delay(u, t)` | a de `u` | valor de `u` há `t` (anel de `t/step` amostras; `t ≤ 15 min`, `J0305`) |

- Os operandos são os sinais do próprio modelo (sem o prefixo do dispositivo), `param.<nome>` e literais. Não se leem sinais de outros dispositivos.
- **Ordem:** em cada `step`, primeiro leem-se as saídas aplicadas, depois avaliam-se as expressões sem estado pela ordem topológica das dependências e, por fim, atualizam-se os estados (`integrate`, `lag`, `delay`) pela ordem do caminho. Um ciclo que não passe por uma função com estado é `J0833`.
- Aritmética em `f64` IEEE 754 com arredondamento para o mais próximo; nada de FMA. O mesmo modelo dá os mesmos bits em x86_64 e aarch64.

#### 2.4.7 `panel`

| Chave | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `kind` | `lamp` \| `momentary` \| `toggle` \| `slider` \| `gauge` | sim | — | tabela abaixo |
| `label` | string ≤ 24 B | não | nome do sinal | |
| `key` | tecla | com `momentary`, `toggle` | — | |
| `keys` | `[diminuir, aumentar]` | com `slider`; opcional em `gauge` | — | |
| `step` | quantidade | com `keys` | — | incremento por tecla |
| `min`, `max` | valores | com `slider` | `limits` do sinal | |
| `pulse` | duração | não | `"150ms"` | `momentary`: tempo a `true` por pressão |
| `order` | inteiro | não | ordem do ficheiro | posição no painel |

| `kind` | Tipos | Direção | Comportamento |
|---|---|---|---|
| `lamp` | `bool`, `enum` | qualquer | mostra; nunca muda o valor |
| `momentary` | `bool` | `input` | cada pressão põe `true` durante `pulse` (um terminal não dá o soltar da tecla); a repetição da tecla prolonga; com `bounce`, o ressalto aplica-se |
| `toggle` | `bool` | `input` | cada pressão inverte |
| `slider` | numérico | `input` | as teclas somam `± step` ao valor, entre `min` e `max`, e substituem o gerador |
| `gauge` | numérico | qualquer | mostra; com `keys`, soma um **desvio** `± step` ao valor gerado (o gerador continua) |

**Teclas:** um carácter imprimível, ou `space`, `enter`, `tab`, `up`, `down`, `left`, `right`, `f1`–`f12`. Reservadas pelo `jk dev`: `q`, `r`, `t`, `?`. Tecla repetida no mesmo site (todos os modelos de um manifesto), tecla reservada, ou `kind` incompatível com o tipo ou a direção: `J0835`.

#### 2.4.8 `[faults]`

```toml
[faults]
"level" = [{ at = "5m", quality = "bad(comm_failure)", for = "20s" }]
panel   = { key = "f", toggle = "bad(comm_failure)", signals = ["temp", "hum"] }
```

**Falhas programadas:** `"<sinal>" = [ falha, … ]`, cada uma com `at` (instante, obrigatório), `for` (duração, obrigatória) e **exatamente um** efeito:

| Efeito | Forma | Semântica |
|---|---|---|
| `quality` | literal de qualidade | a aquisição entrega o sinal com essa qualidade (o valor fica o último bom) |
| `freeze` | `true` | o valor deixa de mudar, com qualidade `good` (o que o `stuck_detect` tem de apanhar) |
| `value` | valor | valor forçado, com qualidade `good` (sensor avariado a mentir) |
| `offset` | quantidade | desvio somado |
| `disconnect` | `true` | só na chave especial `"*"`: o dispositivo inteiro fica `Down` (05 §12): leituras `bad(comm_failure)`, escritas falham |
| `delay` | duração | latência extra |

**Teclas de falha:** a chave `panel` (tabela, ou lista de tabelas) com `key`, um efeito em forma de alternância (`toggle = "<qualidade>"`, `freeze = true` ou `disconnect = true`) e `signals` (omissão: todos os sinais `input` do modelo). Por isso `panel` e `"*"` são nomes reservados em `[faults]`, e um sinal chamado `panel` não pode ter falhas programadas (`J0834`). Falhas sobrepostas no mesmo sinal, sinal inexistente ou `for ≤ 0`: `J0834`.

#### 2.4.9 `[actions]`, `[enums]` e `[implements]`

- `[enums]` e `[implements]` como nos perfis (§2.2.8, §2.2.14). Os alvos dos mapas são sinais do modelo e `"actions.<nome>"`.
- `[actions.<nome>]` tem `args`, `timeout` e `steps`, com os passos `wait`, `fail_if = { signal, equals, message }` e `set = { <sinal> = valor }` (muda sinais do modelo; num sinal com `gen` ou dinâmica, fixa o valor até ao próximo `set` com `release = true`).

#### 2.4.10 `[plant]`: planta em Wasm

Para dinâmicas que não cabem em `[dynamics]`, um componente no mundo `joker:sim/plant` (04 §6). Exclusivo com `[dynamics]` (`J0832`).

| Chave | Tipo | Obrig. | Significado |
|---|---|---|---|
| `component` | caminho `.wasm` ou `oci://…` | sim | |
| `inputs` | lista de sinais `output` do modelo | sim | ordem dos valores entregues à planta |
| `outputs` | lista de sinais `input` do modelo | sim | ordem dos valores devolvidos |
| `config` | tabela | não | passada ao `init`, em CBOR determinístico |

```wit
package joker:sim@0.1.0;

world plant {
    /// Called once, with the [plant] config table encoded as deterministic CBOR.
    export init: func(config: list<u8>, seed: u64) -> result<_, string>;
    /// Advances the plant by dt-ns of virtual time. Values follow the order of
    /// [plant] inputs and outputs; bool is 0.0 or 1.0, enum is the variant value.
    export step: func(dt-ns: u64, inputs: list<f64>) -> result<list<f64>, string>;
}
```

A planta não tem imports (sem relógio, sem aleatoriedade, sem I/O): é determinista por construção. Um `step` que devolva um número de valores diferente de `outputs`, ou um `Err`, põe o dispositivo em `bad(device_fault)` e é `J0837` no `jk check` quando detetável pelo tipo.

#### 2.4.11 Simuladores de classe

Cada classe pode trazer um modelo de simulação em `classes/<pacote>/<nome>.sim.toml`, com o mesmo esquema, `[params]` e um `[implements]` da própria classe. `[instances.<i>] sim = true` usa-o com os `default`; `sim = { … }` passa parâmetros (ref/01). A instância não precisa de dispositivo: a runtime cria um dispositivo `sim` interno com o nome da instância.

```toml
# classes/std/temp_sensor.sim.toml
schema = 1

[params]
mean   = { default = "22 °C" }
amp    = { default = "3 K" }
period = { default = "10m" }

[signals]
temp = { type = "f64", unit = "°C", gen = { kind = "sine", mean = "$mean", amp = "$amp", period = "$period" }, noise = "0.05 K",
         panel = { label = "Temp", kind = "gauge", keys = ["down", "up"], step = "0.5 K" } }

[faults]
panel = { key = "f", toggle = "bad(comm_failure)" }

[implements."std/temp_sensor@1"]
map = { temperature = "temp" }
```

Com várias instâncias simuladas da mesma classe no mesmo site, as teclas colidem; o `jk dev` atribui-lhes teclas livres por ordem de nome e mostra-as no painel, em vez de dar `J0835` (as teclas de um simulador de classe são sugestões).

#### 2.4.12 Exemplo: o ESP32 simulado da trilha

O modelo do ESP32 da bancada (documento 23, §4), tal como a trilha o define na parte 4, §5. Usa sinais com painel (`lamp`, `momentary`, `slider`), ressalto (`bounce`), ruído (`noise`, `resolution`), um gerador (`gen`), dinâmica (`[dynamics]`) e a tecla de falha do painel (`[faults] panel`, §2.4.8), que a parte 5 usa para desligar o ESP32 inteiro.

```toml
# sim/esp.sim.toml (trilha, parte 4, §5)
schema = 1

[model]
description = "ESP32 com o firmware de bancada (documento 23, §4)"

[signals]
led    = { type = "bool", direction = "output", panel = { label = "LED ESP32", kind = "lamp" } }
led_rb = { type = "bool" }
button = { type = "bool", init = false, bounce = "5ms", panel = { label = "BOOT", kind = "momentary", key = "b" } }
touch  = { type = "u64", init = 60, noise = 1, resolution = 1,
           panel = { label = "Toque", kind = "slider", keys = ["down", "up"], step = 5, min = 0, max = 100 } }
rssi   = { type = "i64", gen = { kind = "noise", mean = -55, std = 3 } }
uptime = { type = "f64", unit = "s", init = 0 }
fw     = { type = "u64", direction = "param", init = 1 }

[dynamics]
led_rb = "led"                     # o retorno segue o valor aplicado
uptime = "integrate(1)"            # 1 s por segundo

[faults]
panel = { key = "d", disconnect = true }   # "d" simula o ESP32 desligado (parte 5)
```

---

### 2.5 Cenários

Um cenário é um teste do **site**: o manifesto completo, o núcleo real, o envelope real e relógio virtual (04 §6). Corre com `jk test`; falhar é o código de saída 6 (07 §8).

#### 2.5.1 `[scenario]`

| Chave | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `name` | string ≤ 256 B | sim | — | |
| `description` | string | não | — | |
| `manifest` | caminho relativo à raiz do projeto | não | o manifesto de entrada do `jk test` (`--env` ou `--manifest`, ref/01 §7.7), senão `joker.toml` | |
| `duration` | duração | sim | — | ≤ 30 d; nenhum instante pode passar dela (`J0839`) |
| `seed` | u64 | não | FNV-1a do `name` | semente de todos os modelos (cada modelo deriva a sua) |
| `start` | instante UTC RFC 3339 | não | `"2000-01-01T00:00:00Z"` | UTC do instante 0 |
| `clock` | `synced` \| `holdover` \| `unsynced` | não | `synced` | qualidade inicial do relógio |
| `tags` | lista | não | `[]` | filtro: `jk test --tag` |

#### 2.5.2 Sobreposição local

Um cenário pode ajustar o manifesto sob teste, e só isto:

| Tabela | O que pode mudar |
|---|---|
| `[devices.<d>]` | só dispositivos com driver `sim` (ou `sim-*`): `model`, `params` |
| `[apps.<a>]` | só `params` |

Qualquer outra tabela da raiz é `J0140`. O objetivo é testar o mesmo site com outra planta, e não testar um site diferente. `sim-camera` e os dispositivos de streams são da 1.1 (`J0807`).

#### 2.5.3 Ações

Todas as ações têm `at` (instante, obrigatório). Os alvos são caminhos do site depois dos aliases.

| Ação | Chaves | Semântica |
|---|---|---|
| `[[set]]` | `signal`, **um** de `value` ou `series` (gerador de §2.4.5, com o tempo contado a partir de `at`), `for` (opcional) | substitui a fonte do sinal simulado; com `for`, a fonte original volta no fim |
| `[[press]]` | `signal`, `for` (omissão `"150ms"`) | `true` durante `for`, com o `bounce` do modelo; só em `bool` |
| `[[inject]]` | `signal` (sinal ou seletor) ou `device`, `for` (obrig.), um efeito de §2.4.8 (`quality`, `freeze`, `value`, `offset`, `delay`; `disconnect` só com `device`) | falha durante um intervalo |
| `[[write]]` | `output`, `value`, `ttl` (obrig., A10), `level` (omissão `"manual:4"`), `reason` | escrita manual, como `jk write` |
| `[[release]]` | `output`, `level` | como `jk release` |
| `[[call]]` | `method` (`"<instância>.<método>"`), `args`, `ttl`, `level` | como `jk call` |
| `[[action]]` | `action` (`"<dispositivo\|instância>.<ação>"`), `args`, `key` (omissão: id da ação no cenário) | como `jk action`; o resultado é observável com `result` (§2.5.4) |
| `[[app]]` | `app`, `do` (`stop` \| `start` \| `restart`) | ciclo de vida (trilha 1, §10) |
| `[[ack]]` | `alarm` | reconhece um alarme |
| `[[reset]]` | `output`, `reason` | sai de um trip (`jk reset`) |
| `[[clock]]` | um de `quality` (`clock_quality`) ou `jump` (duração com sinal, `"+5s"`, `"-2s"`) | qualidade do relógio, ou salto do UTC (o monotónico nunca salta) |

`set`, `press` e `inject` sobre sinais que não sejam de um dispositivo `sim`, ou alvos inexistentes: `J0838`.

#### 2.5.4 Expectativas `[[expect]]`

**Quando** (exatamente um):

| Chave | Forma | Semântica |
|---|---|---|
| `at` | instante | o estado depois do último tick do grupo dono com `t_tick ≤ at` |
| `during` | intervalo | em todos os ticks do intervalo |
| `within` | intervalo | em pelo menos um tick do intervalo |
| `before` | instante | ≡ `within = "0s .. <before>"` |

**O quê** (exatamente um sujeito):

| Chave | Forma |
|---|---|
| `signal` | caminho ou seletor (entradas, saídas, `calc.*`, membros de instâncias) |
| `alarm` | nome do alarme |
| `instance` | nome da instância (para `health`) |
| `action` | id de uma `[[action]]` do cenário (para `result`) |
| `metric` | reservado para 1.1 (`J0807`) |

**Predicados** (pelo menos um; `J0840` sem nenhum ou com incompatíveis):

| Predicado | Com | Semântica |
|---|---|---|
| `equals` | `signal` | valor igual, depois da conversão de unidade; `tolerance` (quantidade) opcional, omissão 0 |
| `min`, `max` | `signal` numérico | limites inclusivos; combináveis entre si e com `equals` não |
| `quality` | `signal` | `"bad"` casa só o código; `"bad(comm_failure)"` casa código e motivo |
| `outcome` | `signal` que seja saída | o resultado da escrita no tick (ref/00 §4.4); `"rejected"` casa qualquer motivo |
| `level` | `signal` que seja saída | nível ativo do árbitro, ou `"none"` |
| `toggles` | `signal` `bool`, com `during` | número exato de mudanças no intervalo |
| `toggles_every` | `signal` `bool`, com `during` | tolerância; todos os intervalos entre mudanças completas dentro do intervalo casam (o primeiro e o último, parciais, não contam) |
| `pattern` | `signal` `bool`, com `during` | `"on 100ms, off 100ms, …"`: a sequência repete-se ciclicamente a partir da primeira mudança do intervalo; `tolerance` (duração) opcional, omissão 1 período do grupo dono |
| `state` | `alarm` | `alarm_state` |
| `active` | `alarm` | bool |
| `health` | `instance` | `ok` \| `degraded` \| `failed` |
| `result` | `action` | `ok` \| `err` \| `"err(<motivo>)"` |

`equals`, `min`/`max`, `quality`, `outcome` e `level` combinam-se entre si (todos têm de casar: `equals = "0 kW"` com `outcome = "rejected(interlock)"` é o exemplo 01). `toggles`, `toggles_every` e `pattern` são exclusivos entre si e só com `during`. Uma expectativa que falhe é `J0842`, com o instante, o valor visto e a cadeia de decisão (`jk trace`) no relatório.

#### 2.5.5 Exemplo

```toml
# sim/scenarios/sala.scenario.toml
schema = 1

[scenario]
name     = "fixo abaixo do limite, pisca acima, padrão de falha sem sensor"
manifest = "joker.sim.toml"
duration = "90s"

[[press]]
signal = "painel.botao"
at     = "1s"
for    = "100ms"

[[set]]
at     = "0s"
signal = "painel.pot"
value  = "1.65 V"

[[set]]
at     = "0s"
signal = "sala.temp"
series = { kind = "ramp", from = "22 °C", to = "32 °C", over = "60s" }

[[expect]]
during = "2s .. 15s"
signal = "painel.led"
equals = true

[[inject]]
at      = "70s"
signal  = "sala.temp"
quality = "bad(comm_failure)"
for     = "15s"

[[expect]]
during  = "71s .. 84s"
signal  = "painel.led"
pattern = "on 100ms, off 100ms, on 100ms, off 1700ms"

[[expect]]
at    = "84s"
alarm = "sala_quente"
state = "unknown"
```

---

### 2.6 Casos de conformidade

Um só esquema para as três baterias: a do **núcleo** (16 §8, corre contra o `joker-core` com relógio virtual, no Linux e num MCU), a das **classes** (14 §7.3, corre contra uma instância, em simulação ou num dispositivo) e a dos **drivers** (ref/06 §6, corre contra um driver N, W ou X e um duplo do equipamento). O que difere é a *fixture*; a linha temporal (`do`, `expect`), os literais e os predicados são os mesmos.

#### 2.6.1 Envelope comum

```toml
schema = 1

[suite]          # identificação e fixture global
# fixture do núcleo: [[signals]] [[outputs]] [[writers]] [[atomic]] [[alarms]] [[calc]] [[instances]]
# fixture dos drivers: um ficheiro <driver>.fixture.toml à parte, passado com --fixture (ref/06 §6.2)
[[case]]         # um ou mais casos
```

| Chave de `[suite]` | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `kind` | `core` \| `class` \| `driver` | sim | — | tem de concordar com o sítio do ficheiro (`conformance/<área>/`, `classes/` ou `conformance/drivers/`, `J0845`); `driver`: ações, sujeitos e fixture na ref/06 §6.2–§6.3 |
| `id` | id | sim em `core` e `driver` | em `class`, a referência da classe | prefixo dos ids dos casos nos relatórios |
| `class` | referência de classe | sim em `class` | — | igual ao ficheiro (`classes/std/pump.conformance.toml` → `std/pump@<major da .jkc>`, `J0845`) |
| `spec` | lista de referências (`"16 A7"`, `"01 §7 passo 5"`) | não | `[]` | rastreabilidade |
| `period` | duração | sim em `core` | em `class`, o período do grupo dono da instância; não se aplica a `driver` | período do grupo do caso |
| `hosts` | lista: `linux`, `mcu` | não | ambos | onde o caso tem de dar os mesmos bytes (ADR 0020) |
| `requires` | lista de capacidades | não (`class`, `driver`) | `[]` | a suite toda só corre em instâncias com estas capacidades (em `driver`, capacidades do descritor, ref/06 §2.13) |
| `classes` | lista de caminhos de `.jkc` | não (`core`) | `[]` | as classes das `[[instances]]` da fixture do núcleo (§2.6.2) |
| `tiers` | lista: `n`, `w`, `x` | não (`driver`) | os três | onde a suite de drivers corre |
| `description` | string | não | — | |

| Chave de `[[case]]` | Tipo | Obrig. | Omissão | Significado |
|---|---|---|---|---|
| `id` | id | sim | — | único na suite (`J0805`) |
| `name` | string | não | — | texto livre para humanos |
| `spec` | lista | não | `[]` | |
| `requires` | lista de capacidades | não | `[]` | em `class`: caso ignorado (`J0846`, aviso) se a instância não as tiver; em `driver`: `skip` se o descritor não as declarar |
| `level` | `basic` \| `full` | não (`driver`) | `basic` | nível do caso de driver (ref/06 §6.4) |
| `sim_only` | bool | não (`class`) | `false` | o caso precisa de **posse exclusiva** da instância: que nenhum outro escritor proponha nos membros `out` que o caso observa (por exemplo, "o lease expira e a saída vai para `safe`"). Só corre numa instância simulada; numa instância real, o `jk class verify` não o corre e reporta-o `skipped (sim_only)` (§2.6.3). Em `core` e `driver`, `J0140` |
| `duration` | duração | não | último instante do caso + 1 período | |
| `do` | lista de ações | não | `[]` | §2.6.2 e §2.6.3 |
| `expect` | lista de expectativas | sim | — | §2.6.4 |

Cada caso começa num estado limpo: geração nova, todas as saídas no `safe` (A8), nenhum escritor ativo, alarmes em `normal`, relógio `synced` (ADR 0042).

#### 2.6.2 Fixture e ações do núcleo

A fixture usa a **sintaxe do manifesto** (ref/01), em listas:

| Tabela | Chaves | Notas |
|---|---|---|
| `[[signals]]` | `path`, `type`, `unit`, `quantity`, `enum` (`{ Nome = { … } }` inline), `max_age` (omissão: nunca envelhece) | entradas usadas em interlocks e expressões |
| `[[outputs]]` | `path`, `type`, `unit`, e todas as chaves de `[outputs.*]` (03 §6) | |
| `[[writers]]` | `id`, `level` (`"faixa:n"`), `lease`, `kind` (`cycle` \| `async` \| `manual` \| `bridge` \| `impl`; `proposal` reservado 1.1), `ttl` (obrig. com `manual`, A10) | A1 |
| `[[atomic]]` | `id`, `outputs`, `order`, `commit`, `on_rate` | 02 §7.3 |
| `[[alarms]]` | `id`, e as chaves de `[alarms.*]` | |
| `[[calc]]` | `path` (`calc.<nome>`), `expr` | |
| `[[instances]]` | `name`, `class`, `map`, `capabilities`; a classe vem de `classes = [...]` em `[suite]` (caminhos de `.jkc`) | para os casos de métodos e consultas |

**Ações** em `do` (cada uma com `at`, múltiplo de `period`: `J0843`):

| Ação | Forma | Semântica |
|---|---|---|
| entrada | `{ at, input = "<caminho>", value, quality }` | amostra nova no latch do tick `at` (`quality` omissão `"good"`; `value` pode faltar com `bad`); mantém-se até à próxima |
| proposta | `{ at, propose = "<saída>", writer, value }` | proposta do escritor nesse tick; `async` entra no passo 2, `cycle` no passo 4 (16 §8) |
| método | `{ at, method = "<instância>.<método>", writer, args }` | registo de método (17 §4.1) |
| libertar | `{ at, release = "<saída>", writer }` | fim de uma escrita manual |
| reset | `{ at, reset = "<saída>" }` | fim de trip (E7) |
| relógio | `{ at, clock = "<clock_quality>" }` | |
| geração | `{ at, generation = "same" }` | troca para uma geração idêntica nesse limite de tick (A8) |

As propostas e as libertações de escritores `async`, `manual` e `bridge` entram no passo 2; as de `cycle` e `impl`, no passo 4; em cada passo, pela ordem do caso. Na fronteira de um tick: a troca de geração, as ações `clock`, as amostras `input`, os `reset`, e só depois o latch. A geração nova começa sem níveis ativos e herda o último valor aplicado (A8); as amostras das entradas mantêm-se (ADR 0042).

Escritor, saída, instância ou método desconhecidos: `J0844`.

#### 2.6.3 Ações das classes

A suite corre contra **uma** instância (`jk class verify <classe> --instance <i>`). O executor é um escritor único, `test:<suite>`. Numa instância **simulada** (simulador da classe, ou composta só de sinais `sim`), escreve no nível `control:12`, com `lease` = 3 × período. Cada proposta sua é **mantida**: o executor volta a propô-la em cada tick até a substituir, libertar ou fazer expirar, como faria uma app que escreve em cada tick.

**Numa instância real** (qualquer outra), o executor atua em equipamento, e as regras mudam:

1. Exige `--allow-actuation` (sem ela, `J0640`) e `mode = "dev"` na geração ativa do nó.
2. Antes do primeiro caso, o nó verifica os níveis dos membros `out` da instância: se algum tiver um nível `safety` ou `manual` ativo (de uma app de segurança, de uma pessoa, de uma intenção aprovada), recusa a suite com `J0641` e diz quem o ocupa. Não há `--takeover`: quem está em `manual` ou `safety` tem prioridade sobre um teste.
3. O executor escreve como uma escrita manual do principal que corre o comando (`user:<id>`, `kind = manual`, com `on_behalf_of = test:<suite>` no registo de decisões), no nível **`manual:3`**, com TTL igual à **duração da suite** (o maior `at` dos casos mais o prazo das expectativas, mais 10 s). As propostas são mantidas como acima; o TTL é o lease, e renova-se a cada caso. Se o `jk` ou a ligação caírem a meio, o nível expira sozinho no fim do TTL.
4. **As apps continuam a correr.** Os escritores que o `manual:3` sobrepõe ficam `overridden` durante o teste (e voltam a mandar quando o nível se liberta, no fim da suite ou do TTL), e o envelope do site continua a aplicar-se a cada escrita do executor (interlocks, gamas, taxas). Um `lease_expire` numa instância real liberta o `manual:3` desse membro, e o valor passa ao nível seguinte ativo, e não necessariamente ao `safe`: os casos que esperam o `safe` são ignorados se houver outro nível ativo (`J0846`).
5. Se um nível `safety` ou `manual` de outro principal ficar ativo durante a suite, o caso em curso falha, a suite para, e o executor liberta os seus níveis (o relatório diz porquê).
6. **Os casos `sim_only = true` não correm.** Numa instância real, o executor não tem a posse exclusiva dos membros: as apps continuam a propor por baixo do `manual:3` (regra 4), e um caso que espera "sem ninguém a mandar, a saída vai para o `safe`" veria o valor da app, e não o `safe`. Em vez de o deixar falhar, ou de parar as apps, a suite marca-o: o relatório lista-o como `skipped (sim_only)`, conta-o nos ignorados, e ele não afeta o código de saída. Um caso sem a marca que dependa disso cai na regra 4 (`J0846`, se houver outro nível ativo) ou falha; por isso, quem escreve a suite marca os casos de `lease_expire` cuja expectativa é o `safe` ou `level = "none"`. Na instância simulada, os casos `sim_only` correm como os outros.

| Ação | Forma | Semântica |
|---|---|---|
| método | `{ at, method = "<método>", args }` | chamada pelo escritor de teste |
| proposta | `{ at, propose = "<membro out>", value }` | escrita direta num membro |
| ação | `{ at, action = "<ação>", args, key }` | invocação (async) |
| expirar | `{ at, lease_expire = "<membro out>" }` | o escritor deixa de propor e o lease expira **nesse tick** (o valor efetivo passa a `safe` no latch desse tick, 16 A2 e A6). O `safe` é do site e não da classe: se o `safe` do membro na instância diferir do `@safe_hint` da classe, os casos que usam `lease_expire` nesse membro são ignorados (`J0846`) |
| entrada | `{ at, input = "<membro in>", value, quality }` | só se a instância for simulada (simulador da classe ou composta de sinais `sim`); noutra instância, o caso é ignorado (`J0846`) |

Membro, método ou ação inexistentes: `J0844`.

#### 2.6.4 Expectativas

| Chave | Núcleo | Classe | Forma |
|---|---|---|---|
| quando | `at`, `during` | `at`, `during`, `within`, `before` | §2.5.4; no núcleo, `at` e os extremos de `during` alinhados com `period` (`J0843`) |
| sujeito | `output`, `signal`, `alarm` | `member`, `query`, `health` (bool `true` = verificar a instância), `event`, `action` | caminhos (núcleo) ou nomes relativos à instância (classe) |
| predicados | `equals`, `min`, `max`, `tolerance`, `quality`, `outcome`, `level`, `state`, `active` | os mesmos, mais `health` (`ok` \| `degraded` \| `failed`), `count` (ocorrências de um `event`), `fields` (campos do último evento), `result` | |
| combinadores | `any_of`, `all_of` (listas de expectativas completas) | idem | |

- `equals` numa saída é o **valor aplicado** no tick; o resultado da escrita é `outcome`; o nível ativo é `level` (`"none"` sem escritor ativo).
- Num caso do núcleo, **cada** saída da fixture tem, em cada tick que o caso cobre, um resultado. O executor compara só os ticks e as saídas com expectativa, e o relatório inclui a sequência completa (a porta de determinismo de 16 §7 compara-a byte a byte entre hospedeiros).
- Expectativa falhada: `J0842`.

#### 2.6.5 Exemplos

O caso de 16 §8 neste esquema:

```toml
# conformance/envelope/rate.conformance.toml
schema = 1

[suite]
kind   = "core"
id     = "envelope.rate"
spec   = ["01 §7 passo 5", "16 A7", "16 E2"]
period = "100ms"

[[signals]]
path = "x.f"
type = "f64"
unit = "Hz"

[[outputs]]
path  = "x.p_set"
type  = "f64"
unit  = "kW"
range = ["-100 kW", "100 kW"]
rate  = "10 kW/s"
safe  = "0 kW"

[[writers]]
id    = "a"
level = "optimizer:13"
lease = "2s"
kind  = "cycle"

[[case]]
id   = "clamp_then_safe"
do = [
  { at = "0ms", input = "x.f", value = "50 Hz" },
  { at = "0ms", propose = "x.p_set", writer = "a", value = "50 kW" },
]
expect = [
  { at = "0ms",    output = "x.p_set", outcome = "clamped_rate", equals = "1 kW", level = "optimizer:13" },
  { at = "2200ms", output = "x.p_set", outcome = "none",         equals = "0 kW", level = "none" },
]
```

A suite de 14 §7.3:

```toml
# classes/std/pump.conformance.toml
schema = 1

[suite]
kind  = "class"
class = "std/pump@1"

[[case]]
id     = "start_running_or_failed"
name   = "start → running em ≤ 10 s, ou failed_to_start"
do     = [{ at = "1s", method = "start" }]
expect = [{ any_of = [{ member = "running", equals = true, before = "11s" },
                      { query = "failed_to_start", equals = true, at = "11.1s" }] }]

[[case]]
id       = "stop_zeroes_speed"
requires = ["variable_speed"]
do       = [{ at = "1s", method = "run_at", args = { pct = "60 %" } }, { at = "5s", method = "stop" }]
expect   = [{ member = "speed_set", equals = "0 %", at = "5.2s" }]

[[case]]
id       = "safe_stops_pump"
sim_only = true
do       = [{ at = "1s", method = "start" }, { at = "20s", lease_expire = "run_cmd" }]
expect   = [{ member = "running", equals = false, before = "90s" }]
```

Uma classe só de saídas (`status_light@1`, trilha, parte 8, §3) verifica o **contrato**: a expansão dos métodos, a atomicidade e o `safe` depois do lease. O comportamento da implementação (o pisca) testa-se com os testes da própria implementação. O ficheiro é o da trilha, com a marca `sim_only` no último caso (§2.6.3, regra 6).

```toml
# classes/local/status_light.conformance.toml
schema = 1

[suite]
kind        = "class"
class       = "local/status_light@1"
description = "contrato: expansão dos métodos, blink atómico e limitado, safe depois do lease"

[[case]]
id     = "on_sets_mode"
do     = [{ at = "100ms", method = "on" }]
expect = [{ member = "mode", equals = "on", at = "100ms" }]

[[case]]
id     = "blink_is_atomic_and_clamped"
do     = [{ at = "100ms", method = "blink", args = { period = "10 ms" } }]
expect = [{ all_of = [{ member = "blink_period", equals = "40 ms", at = "100ms" },
                      { member = "mode", equals = "blink", at = "100ms" }] }]

[[case]]
id     = "alert_raises_query"
do     = [{ at = "100ms", method = "alert" }]
expect = [{ query = "alerting", equals = true, within = "100ms .. 200ms" }]

[[case]]
id       = "lease_expiry_goes_off"
sim_only = true          # precisa de posse exclusiva: numa instância real, a app continua a propor
do       = [{ at = "100ms", method = "on" }, { at = "1s", lease_expire = "mode" }]
expect   = [{ member = "mode", equals = "off", level = "none", within = "1s .. 1.1s" }]
```

---

### 2.7 Pacotes de classes

Um pacote de classes é uma pasta `classes/<pacote>/` com um `<pacote>.package.toml` (o último segmento do nome do pacote: `classes/acme/water/water.package.toml`), os `.jkc`, as suites de conformidade e os simuladores de classe.

```
classes/acme/water/
  water.package.toml
  valve.jkc                   (uma classe por ficheiro; `valve@2.jkc` se duas majors coexistirem)
  valve.conformance.toml
  valve.sim.toml
```

- O nome dos ficheiros companheiros é o **mesmo** do `.jkc`: `<nome>.jkc` ↔ `<nome>.conformance.toml` ↔ `<nome>.sim.toml`. Com duas majors na mesma pasta, `<nome>@<major>.jkc` e os companheiros com o mesmo sufixo.
- Um ficheiro `.package.toml` por pasta de pacote, com o nome do último segmento; outro nome, ou dois, é `J0847`.
- O pacote `local` não precisa de `local.package.toml` e nunca se publica.

```toml
# classes/acme/water/water.package.toml
schema = 1

[package]
name        = "acme/water"
description = "Classes de equipamento para redes de água"
license     = "Apache-2.0"
authors     = ["Equipa de automação <automacao@example.com>"]
repository  = "https://git.example.com/acme/joker-classes"
joker       = "1.0"

[dependencies]
"std/pump" = "1.2"
```

| Chave | Tipo | Obrig. | Omissão | Validação |
|---|---|---|---|---|
| `name` | pacote | sim | — | igual à pasta (`J0847`); `std` e `local` reservados (§2.1.2) |
| `description` | string ≤ 1 KiB | não | — | |
| `license` | expressão SPDX | sim para publicar | — | `J0848` |
| `authors` | lista | não | `[]` | |
| `repository`, `homepage`, `docs` | URL | não | — | vão para as anotações OCI |
| `joker` | `"M.m"` | não | `"1.0"` | versão mínima da runtime para todas as classes do pacote |
| `jkc` | inteiro | não | `1` | versão da linguagem `.jkc` |
| `[dependencies]` | tabela `<pacote>/<classe> → "M.m"` | não | `{}` | versão **mínima** de cada classe usada com `use` (dentro da major que o `use` indica); uma classe usada sem entrada aceita qualquer minor; o lock fixa a exata (ref/02) |

O pacote **não tem versão própria**: a unidade de versão e de publicação é a classe (`valve@2.1.0`, documento 17, §9). O `<pacote>.package.toml` dá os metadados partilhados e as dependências. Um `use` para uma classe que não esteja no registo nem no projeto é `J0847`.

---

### 2.8 Publicação no Hub (OCI)

Todos os artefactos de catálogo são artefactos OCI (OCI Image Manifest 1.1, com `artifactType`), assinados pelo papel da ref/00 §7.1 (`profiles`, `boards`, `classes`). Os tipos de media são os da tabela única do ref/00 §7.1 (D13): a forma curta (`joker.profile.v1+toml`) e a completa (`application/vnd.joker.profile.v1+toml`). A tabela abaixo diz só em que camadas vão.

#### 2.8.1 Repositórios e etiquetas

| Artefacto | Repositório | Etiquetas |
|---|---|---|
| perfil | `<registo>/<prefixo>/profiles/<fabricante>/<modelo>` | `<versão>` (imutável) e `<major>` (móvel) |
| placa | `<registo>/<prefixo>/boards/<placa>` | idem |
| classe | `<registo>/<prefixo>/classes/<pacote>/<nome>` | idem |

Exemplo: `registry.example.com/joker/profiles/eastron/sdm630:1.2.0`. O lock fixa sempre o **digest**; as etiquetas são só para descoberta. Republicar uma versão existente com outro conteúdo é recusado (`J0848`).

#### 2.8.2 Tipos de media e camadas

| Artefacto | `artifactType` | Camadas (por esta ordem) |
|---|---|---|
| perfil | `application/vnd.joker.profile.v1+toml` | o ficheiro de perfil (`application/vnd.joker.profile.v1+toml`); opcional: um modelo de simulação do equipamento (`application/vnd.joker.sim.v1+toml`) |
| placa | `application/vnd.joker.board.v1+toml` | o ficheiro de placa (`application/vnd.joker.board.v1+toml`) |
| classe | `application/vnd.joker.class.v1+jkc` | `.jkc` (`application/vnd.joker.class.v1+jkc`); contrato (`application/vnd.joker.class.contract.v1+json`); conformidade (`application/vnd.joker.class.conformance.v1+toml`); opcional: simulador (`application/vnd.joker.sim.v1+toml`); `<pacote>.package.toml` (`application/vnd.joker.class.package.v1+toml`) |
| relatório de conformidade | `application/vnd.joker.conformance-report.v1+json` | o relatório (JSON), com `subject` = o artefacto verificado (*referrer*) |

- A `config` do manifesto OCI é o descritor vazio (`application/vnd.oci.empty.v1+json`, conteúdo `{}`).
- Cada camada leva `org.opencontainers.image.title` com o nome do ficheiro, **com o sufixo do tipo** (`sdm630@1.profile.toml`, `rpi5.board.toml`, `valve.sim.toml`, `water.package.toml`): o `jk` que descarrega uma camada grava-a com esse nome, e a verificação da §2.1.6 aplica-se também ao que vem do registo. Uma camada cujo título não tem o sufixo do seu tipo de media é recusada na publicação (`J0848`) e na instalação (`J0803`).
- As assinaturas (cosign ou notation) e os relatórios de conformidade são *referrers* do artefacto.
- O **digest que o lock fixa** para um perfil ou uma placa é o da camada do ficheiro TOML (sha256 dos bytes do ficheiro). É o mesmo para um ficheiro local e para o artefacto publicado, e por isso mudar de `profiles/` para o registo não muda o lock.

#### 2.8.3 Anotações

No manifesto OCI (valores sempre strings):

| Anotação | Perfil | Placa | Classe | Valor |
|---|---|---|---|---|
| `org.opencontainers.image.title` | sim | sim | sim | nome (`eastron/sdm630`) |
| `org.opencontainers.image.version` | sim | sim | sim | SemVer |
| `org.opencontainers.image.description` | se houver | se houver | se houver | |
| `org.opencontainers.image.licenses` | sim | sim | sim | SPDX |
| `org.opencontainers.image.authors` | se houver | se houver | se houver | separados por vírgula |
| `org.opencontainers.image.source`, `.revision`, `.created`, `.vendor`, `.url`, `.documentation` | opcionais | opcionais | opcionais | |
| `joker.kind` | `profile` | `board` | `class` | |
| `joker.schema` | `1` | `1` | — | |
| `joker.min_runtime` | `joker` | `joker` | `joker` do pacote | `"1.0"` |
| `joker.major` | sim | sim | sim | |
| `joker.driver` | sim | — | — | família |
| `joker.implements` | se houver | — | — | referências de classe separadas por vírgula, ordenadas |
| `joker.arch`, `joker.os`, `joker.mcu_class` | — | sim | — | |
| `joker.contract` | — | — | sim | `sha256:<hex>` do contrato (17 §6) |
| `joker.doc_hash` | — | — | sim | hash de documentação |
| `joker.evidence` | no relatório | — | no relatório | `hardware` \| `sim` |

O prefixo `joker.` é o espaço de nomes das anotações do projeto e nunca muda (ref/00 §1, regra 4).

#### 2.8.4 Verificações de publicação

| Comando | Verifica | Erro |
|---|---|---|
| `jk profile publish` | `jk check` do perfil, `license`, espaço de nomes, `jk profile diff` contra a última versão publicada da mesma major, relatório de conformidade de **cada** `implements` | `J0848`, `J0849` |
| `jk class publish` | `jkc check`, `license` do pacote, `jk class diff` (17 §9), a própria suite a passar contra o simulador da classe (se houver) ou contra uma implementação indicada | `J0848`, `J0849` |
| `jk board publish <ficheiro>` | `jk board check` do ficheiro, `license`, nenhum `# REVER` por resolver, versão ainda não publicada e subida como a §2.3.6 exige, e `tier` `"1"` ou `"2"`: **uma placa `tier = "dev"` nunca se publica no Hub** (nem num registo OCI de organização, que tem as mesmas verificações) | `J0848` |

O relatório de conformidade é produzido por `jk class verify … --report <ficheiro>` e assinado por quem publica. Na 1.0, o Hub aceita `evidence = "sim"` para classes e exige `evidence = "hardware"` para perfis (o perfil diz o que um equipamento real faz). Relatório em falta, falhado, de outra versão ou não assinado: `J0849`.

---

### 2.9 Códigos J0800–J0849

| Código | Gravidade | Mensagem |
|---|---|---|
| `J0800` | error | falta `schema`, ou não é o primeiro par chave/valor do ficheiro |
| `J0801` | error | versão de `schema` não suportada |
| `J0802` | error | o ficheiro exige uma runtime mais recente (`joker`) |
| `J0803` | error | tipo do ficheiro errado: o conteúdo não é do tipo do sufixo do nome, o nome não tem sufixo de tipo, ou quem o refere pede outro tipo |
| `J0804` | error | nome ou versão incoerente com o caminho do ficheiro |
| `J0805` | error | nome inválido ou num espaço de nomes reservado |
| `J0806` | error | literal inválido (duração, quantidade, instante, intervalo, tolerância, qualidade, resultado, estado) |
| `J0807` | error | chave reservada para uma versão futura |
| `J0808` | error | driver do dispositivo incompatível com a família do perfil |
| `J0809` | error | tipo cru inválido ou incompatível com o endereço |
| `J0810` | error | endereço ou código de função inválido para o driver ou para a direção |
| `J0811` | error | sobreposição de pontos ou colisão de caminhos no perfil |
| `J0812` | error | ponto ou passo num endereço `forbidden` |
| `J0813` | error | grupo coerente não cabe num pedido |
| `J0814` | error | enum ou bit inexistente, duplicado ou fora da gama |
| `J0815` | error | escala, offset, unidade ou tipo do sinal incoerentes com o tipo cru |
| `J0816` | error | `limits`, `stuck_detect` ou `max_age` inválidos (`max_age` tem de ser maior do que o `poll`, também nos ajustes do manifesto) |
| `J0817` | error | `poll` inválido |
| `J0818` | error | `quirks` desconhecido ou fora da gama para a família |
| `J0819` | error | `config` do dispositivo fora do esquema do perfil |
| `J0820` | error | fallback do perfil mal declarado |
| `J0821` | error | passo de ação inválido |
| `J0822` | error | argumentos de ação em falta, a mais ou com tipo errado |
| `J0823` | error | classe de `implements` desconhecida |
| `J0824` | error | mapeamento de `implements` inválido (direção, adaptador, capacidade não declarada) |
| `J0825` | error | descoberta ou `[profile] identity` mal declaradas |
| `J0826` | error-in-prod | firmware do equipamento fora da gama declarada pelo perfil |
| `J0827` | error | placa desconhecida em `jk build --target` (no manifesto, `[target] board` desconhecida é `J0172`, ref/01) |
| `J0828` | error | recurso inexistente na placa (linha, porta, barramento, acelerador) |
| `J0829` | error | perfil de placa incoerente (arquitetura, sistema, CPUs, pinos) |
| `J0830` | error | capacidades do firmware acima da memória da placa |
| `J0831` | error | gerador desconhecido ou com parâmetros inválidos |
| `J0832` | error | sinal simulado com mais do que uma fonte |
| `J0833` | error | dinâmica inválida (função fora de `[dynamics]`, ciclo algébrico, estado inicial em falta) |
| `J0834` | error | falha simulada inválida |
| `J0835` | error | painel inválido (tecla repetida ou reservada, tipo incompatível) |
| `J0836` | error | parâmetro de simulação desconhecido, sem valor ou com dimensão errada |
| `J0837` | error | planta Wasm incompatível com o modelo |
| `J0838` | error | ação de cenário sobre um alvo inexistente ou não simulado |
| `J0839` | error | instante ou intervalo fora do cenário ou invertido |
| `J0840` | error | expectativa sem predicado ou com predicados incompatíveis |
| `J0841` | error | seletor sem correspondências |
| `J0842` | error | expectativa falhou |
| `J0843` | error | instante de um caso do núcleo não alinhado com o período |
| `J0844` | error | referência desconhecida num caso (escritor, saída, instância, membro, método) |
| `J0845` | error | suite não corresponde ao ficheiro ou à classe |
| `J0846` | warning | caso ignorado: a instância não tem a capacidade ou não é simulável |
| `J0847` | error | pacote de classes inválido |
| `J0848` | error | publicação recusada: metadados em falta, espaço reservado, versão já publicada ou incompatível sem subir a major, ou placa com `tier = "dev"` |
| `J0849` | error | publicação sem relatório de conformidade válido |

Em `mode = "dev"`, os `error-in-prod` aparecem como `W08xx` com o mesmo número (16 §9).

---

## 3. Comportamento esperado

**Validação.** Tudo o que está na §2 é verificado no `jk check` sem I/O: os ficheiros de catálogo e de simulação referidos pelo manifesto, os cenários de `sim/scenarios/` e as suites. O `plan` do driver (puro) valida o endereçamento e a coerência; um erro do `plan` leva o código de §2.9 e a localização no ficheiro. A ordem de validação é: TOML → `schema`/`joker` → chaves (`J0140`) → literais → referências → regras de domínio. Os erros são todos reportados, não só o primeiro, por ordem de ficheiro e linha.

**Resolução.** Um perfil `<nome>@<major>` resolve-se por esta ordem: `profiles/` do projeto (o ficheiro `profiles/<nome>@<major>.profile.toml`; uma placa, `boards/<placa>.board.toml`; §2.1.6); as entradas do caminho de pesquisa da configuração do cliente, por ordem (`defaults.search`, ref/02 §2.3.2), em que `"builtin:"` são os embutidos (`joker/…`) e, sem ela na lista, os embutidos vêm no fim. Por omissão, o caminho é `["builtin:", "<Hub oficial>"]` (nos exemplos, `oci://hub.example.com/joker`). Dois candidatos com o mesmo nome e major em fontes diferentes é `J0805` (ambiguidade é erro), a não ser que o lock fixe um. O mesmo vale para placas e classes.

**Aquisição.** A ordem de aplicação de um ponto cru, no estágio de aquisição (01 §3.3): descodificação (tipo, `word_order`, `byte_order`, `bit`, `invert`) → `invalid` → `scale_factor`, `scale`, `offset` → conversão para a unidade canónica → NaN (`bad(nan)`) → `limits` (`bad(out_of_range)`) → `enum` (`bad(out_of_range)` sem variante) → `deadband` → `stuck_detect` → qualidade do link (05 §12). O primeiro que marca `bad` decide o motivo.

**Simulação.** O driver `sim` é determinista: com o mesmo manifesto, os mesmos modelos, a mesma semente e o mesmo cenário, a sequência de amostras e de resultados é igual bit a bit em qualquer hospedeiro. O relógio é o virtual do `jk test` (ADR 0007); no `jk dev`, o mesmo passo corre em tempo real. As ações de um cenário no mesmo instante aplicam-se pela ordem do ficheiro, antes do latch dos grupos com `t_tick = at`.

**Conformidade.** Os valores esperados são escritos à mão (16 §8, regra do Cravo). O executor não "corrige" expectativas nem interpola: um caso passa ou falha. No núcleo, a sequência completa de resultados de cada caso é o artefacto que se compara entre Linux e MCU.

**Compatibilidade.** `schema = 1` cobre toda a série 1.x. Numa minor, só se acrescentam chaves opcionais, valores de enum no fim e secções novas (ref/00 §1). Um ficheiro que use uma chave de uma minor declara `joker = "1.<minor>"`; o `jk check` avisa se a chave usada exigir uma minor acima da declarada, para que o erro apareça a quem escreve e não a quem instala (§4, S1). Um `schema = 2` só na v2, com `jk migrate`.

**Segurança.** Ações de perfil não passam pelo envelope (são ações), mas passam pelo permissivo e pelo papel do manifesto (17 §4.2) e ficam no registo de decisões. Os passos `[connect]` e `[relinquish]` também ficam. Um perfil não pode declarar `safe`: só `safe_hint` (ADR 0006). `jk class verify` contra uma instância que não seja simulada exige `--allow-actuation` (`J0640`), só corre com `mode = "dev"` no nó, recusa-se com um nível `safety` ou `manual` ativo nas saídas da instância (`J0641`), e o escritor de teste escreve em `manual:3` com TTL igual à duração da suite e passa pelo árbitro e pelo envelope do site como qualquer outro (§2.6.3).

**Dependências com outras referências** (verificadas campo a campo nesta rodada):

| Referência | O que é coerente |
|---|---|
| ref/01 | `[devices.<d>]`: `profile` (`<nome>@<major>`, ou `sunspec` ≡ `joker/sunspec@1`), `model` e `params` (driver `sim`, §2.4), `config` (§2.2.10), `discover` (§2.2.15), `fallback.configure` (§2.2.11); `[instances.<i>] sim = true \| tabela` (§2.4.11); `[target] board = "nome" \| "nome@major"` (§2.3.6); ajustes de sinais com a mesma gramática (§2.2.4, incluindo `scale = { from, to }`); chaves de endereço dos drivers sem perfil iguais às de §2.2.5 (anexo A da ref/01); IO fixo das placas com `fallback` |
| ref/02 | lock: `[profiles."<nome>@<major>"]` e `[boards."<valor de board>"]` com `resolved` e `digest` = SHA-256 dos bytes do ficheiro (§2.8.2); `[classes."<ref>"]`; `[discovered.<d>]` com `models` nos perfis SunSpec; `[fallback.<d>]` campo a campo com §2.2.11; `[mcu.<d>] capacity` com os nomes de `[capacities]`; caminho de pesquisa de registos (`defaults.search`) |
| ref/04 | mundo `joker:sim/plant` (§2.4.10) em `wit/joker-sim.wit`; `joker-test` usa os mesmos literais de §2.1.3 |
| ref/06 | o esquema `address` do descritor = as chaves de §2.2.5; o esquema `quirks` = §2.2.6; o esquema `native` = a tabela `native` de §2.2.12; o `plan` devolve os códigos J08xx de §2.9; `protocols` do descritor contém a família de §2.2.2; a gama J0850–J0899 é dos drivers (ref/00 §8) |
| ref/08 | `[capacities]`, `[areas]`, `[irq]`, `reset_safe`, `stack`, `fpu`, `[watchdog]` e `[link] default` das placas MCU (§2.3.1, §2.3.3) = o que o `jk build` e o firmware usam; `J0710` compara com `[capacities]` |
| ref/00, ref/09, ref/10 | tipos de media e papéis da ref/00 §7.1; a API e o agente expõem as anotações de §2.8.3 sem as mudar |

---

## 4. Estresse

**S1 — Um perfil do Hub usa uma chave da 1.3; o nó está na 1.1.** Sem mais nada, o nó daria `J0140` "chave desconhecida", que parece um erro de escrita. **Quebrou.** **Correção incorporada:** `joker = "1.3"` no ficheiro dá `J0802` com a versão pedida; a anotação `joker.min_runtime` deixa o Hub e o `jk lock` escolherem a última versão compatível; o `jk check` de quem escreve avisa se a chave usada exigir uma minor acima da declarada.

**S2 — Um inversor SunSpec com a escala num registo à parte.** Se `W` e `W_SF` viessem de pedidos diferentes, podia ler-se o valor novo com a escala antiga (05 §3). **Aguenta:** `scale_factor` torna o par coerente por construção, e o `plan` falha com `J0813` se não couber num pedido.

**S3 — Dois perfis definem `PcsState` com números diferentes.** `permit = "pcs.state == PcsState.fault"` podia resolver para o enum errado. **Aguenta:** o enum tem âmbito de perfil, o literal resolve-se pelo tipo do operando, e um literal sem operando tipado com dois candidatos é `J0303` com a lista.

**S4 — Quem escreve copia `Bad(CommFailure)` do `jk status`.** Recusar seria pedante; aceitar tudo seria perder o controlo da gramática. **Aguenta:** a forma de apresentação é um sinónimo exato com uma tradução total (§2.1.3), o `jk fmt` normaliza, e qualquer outra grafia é `J0806`.

**S5 — `at = "2m+1.2s"` contra "não há compostos" (ref/00 §3.1).** **Quebrou** a convenção, e com razão: num cenário o instante lê-se como "depois da falha, mais 1,2 s". **Correção incorporada:** somas com `+` só nas posições de instante da linha temporal (cenários e casos); em todo o lado, continuam proibidas. O ref/00 §3.1 já o diz (resolvido).

**S6 — O mesmo cenário dá resultados diferentes no portátil e no CI.** Ruído, ressalto e `walk` usam aleatoriedade. **Aguenta:** semente fixa derivada do nome do cenário e do caminho do dispositivo, Euler explícito em `f64` sem FMA, ordem de avaliação definida (§2.4.6), plantas Wasm sem imports. `jk test --seed` reproduz uma falha encontrada por `--fuzz`.

**S7 — `jk class verify pump@1 --instance vfd1` num site real.** Arranca uma bomba a sério. **Quebrou.** **Correção incorporada:** `--allow-actuation` obrigatório fora de instâncias simuladas, só em `mode = "dev"`, e o escritor de teste passa pelo envelope do site (interlocks e gamas continuam a mandar).

**S8 — Uma classe só de saídas (`status_light@1`).** Não há nada para observar além dos próprios `out`. **Aguenta, com fronteira declarada:** a conformidade verifica o contrato (expansão, atomicidade, `safe` após o lease); o comportamento de cada implementação é dos testes dela (§2.6.5).

**S9 — Quinze medidores num RS-485 com perfis sem `latency`.** O orçamento do barramento (`J0370`) dependia de um número que ninguém escreveu. **Aguenta:** `quirks.latency` tem omissão de 10 ms (o valor de 05 §5), e o `jk profile test` mede a latência real e sugere o valor.

**S10 — Republicar `sdm630:1.2.0` com um registo corrigido.** Um site com o lock antigo passaria a ler outro mapa com a mesma versão. **Aguenta:** versões imutáveis no Hub (`J0848`), digest do ficheiro no lock, e o `jk profile diff` obriga a subir pelo menos a patch.

**S11 — Um botão momentâneo num terminal.** Os terminais não dão o soltar da tecla, e "`true` enquanto premido" não se consegue implementar. **Quebrou** o que 04 §6 sugeria. **Correção incorporada:** `momentary` é um impulso de `pulse` (150 ms) por pressão, prolongado pela repetição da tecla; o ressalto aplica-se por cima.

**S12 — Um variador CANopen com 3 000 parâmetros.** Um perfil com todos em ciclo esgotava o barramento. **Aguenta:** `direction = "param"` implica `poll = "on_demand"` (05 §3), o limite de 4 096 sinais por perfil cobre um OD real, e o `jk profile import eds` gera-os já assim.

---

## 5. Decisões e alternativas rejeitadas

| Decisão | Alternativa rejeitada | Porquê |
|---|---|---|
| o tipo do ficheiro está no sufixo do nome (`.profile.toml`, `.board.toml`, `.sim.toml`, …), verificado contra o conteúdo (`J0803`), e no Hub no tipo de media (ADR 0026) | o tipo vem de quem o refere (a decisão anterior desta referência); uma chave `kind` em cada ficheiro | com o tipo só em quem refere, um `esp.toml` não se percebe numa listagem nem numa revisão; uma chave `kind` pode divergir do nome e não aparece numa listagem. O sufixo diz o tipo sem abrir o ficheiro e deixa os editores escolher o esquema |
| sem herança entre perfis na 1.0 (`extends` reservado) | `base = "eastron/sdm630@1"` | um perfil lê-se e revê-se sozinho; as famílias de modelos resolvem-se com o importador; a herança complica o lock e o `jk profile diff` |
| um só esquema de conformidade (D10), com `do`/`expect` e `at`, também para os drivers (ref/06 §6) | manter `[[ticks]]` no núcleo, `do`/`expect` nas classes e `[[steps]]` nos drivers | dois vocabulários para a mesma coisa; com `do`/`expect`, os literais e os predicados são os mesmos em cenários, casos do núcleo, das classes e dos drivers. **Exceção declarada:** nas suites `driver`, e só aí, os instantes aceitam o termo `timeout` (o da fixture), porque o mesmo caso corre contra drivers com timeouts diferentes e, no tier X, com os prazos escalados |
| `equals` para o valor aplicado, em todo o lado | `applied` no núcleo | o valor de uma saída **é** o aplicado; um nome só |
| forma de apresentação aceite como sinónimo (`Bad(CommFailure)`) | só `snake_case` | o que a CLI mostra tem de poder ser colado; a tradução é total e o `jk fmt` normaliza |
| somas de instantes (`2m+1.2s`) só na linha temporal | escrever `121.2s` | legibilidade de testes; fora da linha temporal, a regra de ref/00 mantém-se |
| o pacote de classes não tem versão | versionar o pacote e as classes | a classe já tem versão e hash de contrato; uma segunda versão só criaria conflitos |
| um artefacto OCI por versão de classe | um artefacto por pacote | o lock e a compatibilidade são por classe (17 §9) |
| o digest de perfis e placas no lock é o do ficheiro | o do manifesto OCI | é igual para um ficheiro local e para o publicado |
| placas versionadas (`version`, `rpi5@1`) | placas sem versão | uma placa também muda (um caminho de `/dev`, uma partição) e o lock tem de o fixar |
| `fallback` nas linhas de IO da placa, em vez de `safe` | `safe` (05 §11) | o `safe` é do manifesto (ADR 0006); o que a placa sabe é o que o hardware faz sozinho, que é um fallback (M1) |
| `momentary` como impulso | "enquanto premido" | impossível num terminal (S11) |
| `health = "failed"` sobre instâncias, nos cenários | ler a qualidade dos membros um a um | a saúde é da classe (17 §4.5); o teste deve usar a mesma regra |

---

## 6. Marcações de versão

| Tema | 1.0 | Reservado |
|---|---|---|
| perfis | tudo o que está na §2.2 com as famílias marcadas 1.0 | `kind = "block" \| "stream"` (1.1); famílias `ethercat`, `bacnet`, `iec104`, `iec61850`, `dlms` (1.2); `extends` (1.x); várias instâncias implícitas por dispositivo (`instances`, 1.x); descoberta `canopen_lss`, `opcua_browse` (1.x) |
| placas | Linux e MCU (§2.3) | `[accel]`: aceite e validado já, usado pelo ML na **1.1** (Preparado para ML); Wasm em MCUs M2 (`capacities.wasm`, 1.4) |
| simulação | geradores, dinâmicas, falhas, painel, ações, `implements`, `[plant]`, simuladores de classe | `sim-camera` e fontes de streams e blocos (1.1); simulador que **serve** o protocolo de um perfil (`serve = { profile, protocol }`, para o `jk profile test` contra simuladores do Hub, 1.5) |
| cenários | §2.5 | `metric` (1.1, com os pipelines e o ML; exemplo 02); `[[inject]]` sobre streams (1.1) |
| conformidade | §2.6 | escritor `kind = "proposal"` (1.1, M18); expectativas sobre saídas de modelos (1.1, M22) |
| Hub | §2.8 com o Hub mínimo (registo OCI + verificações) | `application/vnd.joker.model.v1+jkm` (1.1, já reservado em 11); Hub público com evidência obrigatória por simulador certificado (1.5) |

---

## 7. Cobertura

Extração por script, regenerada depois da reescrita da trilha (onze partes sobre a bancada do documento 23): todos os blocos ` ```toml ` de todos os `.md` do projeto fora de `docs/ref/` (125 blocos, todos TOML válido), classificados por tipo pelo cabeçalho e pelas tabelas de topo. 10 são perfis (inteiros ou excertos), 6 placas, 6 modelos de simulação, 9 cenários e 3 casos de conformidade; os manifestos e os locks são da ref/01 (anexo B) e da ref/02. As chaves normalizam-se como no anexo B da ref/01 (nomes livres → `*`, elementos de listas → `[]`), e as folhas com o mesmo pai juntam-se numa linha: 67 linhas. "Onde aparece" dá o documento e a secção do bloco (`trilha 4 §5` é a parte 4, secção 5). Um segundo passo confirmou que cada folha usada na trilha aparece na secção indicada; nenhuma ficou sem definição. A única chave da trilha que não é da 1.0 é `implements.*.instances` (trilha 7 §4), e está lá de propósito, como o erro `J0807` que o `jk check` dá.

Os achados A1–A24 (§9) foram escritos contra o corpus de então, que incluía a trilha antiga (sete partes, com o `painel`, a `sala`, o ADS1115 e o XY-MD02); ficam como histórico, e as tabelas abaixo já não os citam.

### 7.1 Perfis

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| `actions.*.steps.[].wait` | 05 §6 | §2.2.12 |
| `actions.*.steps.[].write.{at,fc,value}` | 05 §6 | §2.2.12 |
| `coherent.*` | 05 §6 | §2.2.9 |
| `defaults.{byte_order,fc,poll,type,word_order}` | 05 §6; 23 §4; trilha 4 §4.2 | §2.2.3 |
| `enums.*.*` | 05 §6 | §2.2.8 |
| `fallback.{applies_to,timeout,value}` | 05 §6; 23 §4; trilha 4 §4.2 | §2.2.11 |
| `fallback.timeout_register.{at,fc,type,unit}` | 05 §6 | §2.2.11 |
| `fallback.value_register.{at,fc,scale,type,unit}` | 05 §6 | §2.2.11 |
| `implements.*.{capabilities,fault,fault_code,hours,reset,run_cmd,running,speed}` | 05 §6; 14 §5.1; 17 §5.1; propostas/idl-classes | §2.2.14 |
| `implements.*.instances.*.pressed` | trilha 7 §4 | §2.2.14 |
| `implements.*.map.*.fields.code` | 17 §5.1 | §2.2.14 |
| `implements.*.map.*.{offset,on_rise,scale,signal}` | 17 §5.1; trilha 7 §4 | §2.2.14 |
| `implements.*.speed_set.{scale,signal}` | propostas/idl-classes | §2.2.14 |
| `implements.*.tripped.fields.{at_speed,code}` | propostas/idl-classes | §2.2.14 |
| `implements.*.tripped.on_rise` | propostas/idl-classes | §2.2.14 |
| `profile.{description,docs,driver,license,name,vendor,version}` | 05 §6; 23 §4; trilha 4 §4.2 | §2.2.2 |
| `profile.identity.firmware` | trilha 4 §4.2 | §2.2.2 |
| `profile.quirks.{max_gap,max_regs,min_interval}` | 05 §6 | §2.2.6 |
| `schema` | 05 §6; 23 §4; trilha 4 §4.2 | §2.1.1 |
| `signals.*.{at,deadband,description,direction,enum,fc,label,limits,monotonic,quantity,readback,safe_hint,scale,type,unit}` | 05 §6; 23 §4; trilha 4 §4.2 | §2.2.4, §2.2.5 |
| `signals.*.bits.{0,1,5}` | 05 §6 | §2.2.4, §2.2.5 |

### 7.2 Placas

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| `adc.*` | 23 §7 | §2.3.3 |
| `areas.*.{offset,size}` | 23 §7 | §2.3.3 |
| `board.{arch,boot_cmdline,clock,cpus,flash,fpu,kernel,mcu_class,memory,model,name,os,reset_safe,stack,tier,vendor,version}` | 05 §11; 23 §7; trilha 6 §5.2; trilha 6 §5.3 | §2.3.1 |
| `board.recommended.{bg,io,ml,rt}` | 05 §11; 23 §7; trilha 6 §5.2; trilha 6 §5.3 | §2.3.1 |
| `boot.{bootloader,slots}` | 23 §7 | §2.3.3 |
| `capacities.*` | 23 §7 | §2.3.3 |
| `gpio.*` | 23 §7; trilha 6 §5.3 | §2.3.2 |
| `i2c.*` | 23 §7 | §2.3.2 |
| `io.*.{direction,driver,fallback,led,line,thermal}` | 05 §11; 23 §7; trilha 6 §5.2; trilha 6 §5.3 | §2.3.2 |
| `irq.priorities` | 23 §7 | §2.3.3 |
| `link.*.{baud,port,transport}` | 23 §7 | §2.3.3 |
| `net.*` | 23 §7; trilha 6 §5.3 | §2.3.2, §2.3.3 |
| `pins.*.{functions,note,touch_channel}` | 23 §7 | §2.3.3 |
| `probe.{protocol,tool}` | 23 §7 | §2.3.3 |
| `schema` | 05 §11; 23 §7; trilha 6 §5.3 | §2.1.1 |
| `serial.*` | 05 §11; 23 §7; trilha 6 §5.2 | §2.3.2 |
| `spi.*` | 23 §7 | §2.3.2 |
| `uart.*.{max_baud,rx,tx}` | 23 §7 | §2.3.3 |
| `watchdog.{device,kind,timeout}` | 05 §11; 23 §7; trilha 6 §5.3 | §2.3.2, §2.3.3 |

### 7.3 Simulação

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| `dynamics.*` | 04 §6; exemplo 00; trilha 3 §1; trilha 4 §5 | §2.4.6 |
| `faults.*.[].{at,for,quality}` | 04 §6 | §2.4.8 |
| `faults.panel.{disconnect,key}` | trilha 4 §5; trilha 5 §4 | §2.4.8 |
| `model.description` | trilha 1 §3; trilha 3 §1; trilha 4 §5 | §2.4.2 |
| `schema` | 04 §6; exemplo 00; trilha 1 §3; trilha 3 §1; trilha 4 §5 | §2.1.1 |
| `signals.*.{bounce,direction,init,label,noise,resolution,type,unit}` | 04 §6; exemplo 00; trilha 1 §3; trilha 3 §1; trilha 4 §5 | §2.4.4 |
| `signals.*.gen.{amp,kind,mean,period,std}` | 04 §6; exemplo 00; trilha 4 §5 | §2.4.5 |
| `signals.*.panel.{key,keys,kind,label,max,min,step}` | trilha 1 §3; trilha 3 §1; trilha 4 §5 | §2.4.7 |

### 7.4 Cenários

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| `ack.[].{alarm,at}` | trilha 3 §7 | §2.5.3 |
| `app.[].{app,at,do}` | trilha 1 §10 | §2.5.3 |
| `devices.*.{defect_rate,driver,frames,late_rate}` | exemplo 02 | §2.5.2 |
| `expect.[].{active,alarm,at,during,equals,level,max,metric,outcome,quality,signal,state,toggles,toggles_every,within}` | exemplo 00; exemplo 01; exemplo 02; trilha 1 §10; trilha 3 §7; trilha 4 §8; trilha 5 §4; trilha 8 §7 | §2.5.4 |
| `inject.[].{at,device,disconnect,for,quality,signal}` | exemplo 00; trilha 3 §7; trilha 5 §4 | §2.5.3 |
| `press.[].{at,for,signal}` | trilha 4 §8; trilha 5 §4 | §2.5.3 |
| `scenario.{duration,manifest,name,tags}` | exemplo 00; exemplo 01; exemplo 02; trilha 1 §10; trilha 3 §7; trilha 4 §8; trilha 5 §4 | §2.5.1 |
| `schema` | exemplo 00; exemplo 01; exemplo 02; trilha 1 §10; trilha 3 §7; trilha 4 §8; trilha 5 §4 | §2.1.1 |
| `set.[].{at,signal,value}` | exemplo 01; trilha 3 §7; trilha 5 §4 | §2.5.3 |

### 7.5 Conformidade

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| `case.[].do.[].args.{pct,period}` | 14 §7.3; trilha 8 §3 | §2.6.2, §2.6.3 |
| `case.[].do.[].{at,lease_expire,method,propose,value,writer}` | 14 §7.3; 16 §8; trilha 8 §3 | §2.6.2, §2.6.3 |
| `case.[].expect.[].all_of.[].{at,equals,member}` | trilha 8 §3 | §2.6.4 |
| `case.[].expect.[].any_of.[].{at,before,equals,member,query}` | 14 §7.3 | §2.6.4 |
| `case.[].expect.[].{at,before,equals,level,member,outcome,output,query,within}` | 14 §7.3; 16 §8; trilha 8 §3 | §2.6.4 |
| `case.[].{id,name,requires,sim_only}` | 14 §7.3; 16 §8; trilha 8 §3 | §2.6.1 |
| `outputs.[].{path,range,rate,safe,type,unit}` | 16 §8 | §2.6.2 |
| `schema` | 14 §7.3; 16 §8; trilha 8 §3 | §2.1.1 |
| `suite.{class,description,id,kind,period,spec}` | 14 §7.3; 16 §8; trilha 8 §3 | §2.6.1 |
| `writers.[].{id,kind,lease,level}` | 16 §8 | §2.6.2 |

### 7.6 Chaves de manifesto que referem estes ficheiros

| Chave do manifesto | Onde aparece | Nesta referência |
|---|---|---|
| `devices.*.profile` | 03 §5; 08 §1.2; 08 §3.2; 08 §4.2; 08 §5.2; 14 §5.1; 23 §6.3; 23 §6.4; 23 §6.5; exemplo 00; exemplo 01; exemplo 03; trilha 4 §9; trilha 6 §2; trilha 6 §6; trilha 9 §9 | §2.2 (`<nome>@<major>`; `sunspec` ≡ `joker/sunspec@1`, §2.2.15) |
| `devices.*.discover` | 03 §5; 08 §1.2; exemplo 01 | §2.2.15 |
| `devices.*.fallback.configure` | 03 §5; 08 §1.2; exemplo 01 | §2.2.11 |
| `devices.*.config` | 03 §5 | §2.2.10 |
| `devices.*.model` | 03 §5; exemplo 00; trilha 1 §4; trilha 4 §5 | §2.4 (driver `sim`) |
| `target.board` | 03 §3; 08 §1.2; 23 §6.3; 23 §6.4; exemplo 00; exemplo 01; exemplo 02; exemplo 03; trilha 2 §7; trilha 6 §2; trilha 6 §6; trilha 9 §3 | §2.3 (`nome` ou `nome@major`, §2.3.6) |
| `devices.*.chip` | 08 §2.2; 08 §5.2; exemplo 02 | §2.3.2 (`[gpio]`) |
| `devices.*.signals.*.line` | 08 §2.2; 08 §5.2; 09 §12.2; exemplo 02; trilha 9 §3 | §2.3.2, §2.3.3 (pinos de MCU) |
| `devices.*.signals.*.touch` | trilha 9 §3 | §2.3.3 (`touch_channel`) |

### 7.7 Pacotes e Hub

| Chave | Onde aparece | Nesta referência |
|---|---|---|
| tipos de media `joker.profile.v1+toml`, `joker.board.v1+toml`, `joker.class.v1+jkc` | 20 §3.2; ref/00 §1 | §2.8.2 — **achado A14** |
| `<pacote>.package.toml` e anotações | — (não existia) | §2.7, §2.8.3 |

---

## 8. Compatibilidade de `schema = 1` com os exemplos

- **Sintaticamente compatível.** Nenhum ficheiro de catálogo ou de teste dos exemplos usava uma chave `schema` na raiz, nem uma chave na raiz antes da primeira tabela; acrescentar `schema = 1` no topo deu TOML válido em todos.
- **Aplicado (D11, achado A1).** Os ficheiros completos de catálogo e de teste dos exemplos, da trilha e dos documentos 04, 05, 14 e 16 abrem agora com `schema = 1` (depois do comentário com o caminho). Os excertos marcados "acrescentar" ou "excerto" não o repetem, porque não são ficheiros inteiros.
- **"Primeiro par chave/valor"** (ref/00 §1, D11): os exemplos abrem com um comentário com o caminho (`# sim/tank.sim.toml`), e é permitido; o `schema = 1` vem logo a seguir.

---

## 9. Achados sobre os documentos existentes

Os exemplos e a trilha são ilustrativos (precedência de `docs/README.md`; D1, ADR 0023); onde contradizem esta referência, corrige-se o exemplo. Foram todos aceites (documento 22, §7); onde uma decisão D1–D23 os resolveu, a linha di-lo.

| # | Documento | Achado | Mudança |
|---|---|---|---|
| A1 | todos os exemplos da §8 | falta `schema = 1` | acrescentar a seguir ao comentário do caminho (**D11**) |
| A2 | 16 §8 | `[case]` + `[[ticks]]` com `t`, `applied`, `"Good"`, `"Clamped(Rate)"` | `[suite]` + `[[case]]` com `do`/`expect`, `at`, `equals`, `"good"`, `"clamped_rate"` (§2.6.5) (**D10**) |
| A3 | 14 §7.3 | ficheiro `pump@1.conformance.toml`; casos sem `id`; `jk class publish classes/local/station@1.toml` | `pump.conformance.toml`; `id` em cada caso; `jk class publish classes/local/station.jkc` |
| A4 | 04 §6; exemplos 00, 01; trilha antiga 3–5; 16 §8 | literais `Bad(CommFailure)`, `Rejected(Interlock)`, `Accepted`, `Unknown` | aceites como sinónimos; reescrever na forma canónica (`bad(comm_failure)`…) (**D10**, ref/00 §2) |
| A5 | exemplos 00, 01; ref/00 §3.1 | instantes `2m+1.2s` contra "não há compostos" | **Resolvido:** o ref/00 §3.1 admite somas só na linha temporal |
| A6 | 05 §11 | `io.relay.safe = false` numa placa | `fallback = false` (substitui o `safe_hint` do achado C16 da ref/01, que fica resolvido assim) |
| A7 | 05 §4 | `coherent = ["ac.p", "ac.q", "ac.pf"]` como lista direta | `[coherent] ac = ["ac.p", "ac.q", "ac.pf"]` |
| A8 | 04 §6; trilha antiga 5 | `[faults] panel` partilha a tabela com nomes de sinais | documentar `panel` e `"*"` como reservados em `[faults]` |
| A9 | exemplo 02 §4 | `[devices.cam0] driver = "sim-camera"` e `metric` num cenário | marcar como 1.1 (**D5**, ADR 0019) |
| A10 | 03 §5; 08; exemplo 01 | `profile = "sunspec"` sem major | forma curta de `joker/sunspec@1`; **aplicado** na ref/01 §5.8 e na ref/02 §2.1.5 (o lock usa a forma completa) |
| A11 | propostas/idl-classes §5 | `"std.pump@1"` e membros direto na tabela | nota de obsoleto a apontar para 17 §5.1 |
| A12 | 04 §6; 02 §10 | `integrate(…, min = 0 m, max = 3 m)` usa argumentos nomeados, que a gramática de 02 §10 não tem | 04 §6 remete para §2.4.6 (extensão só em `[dynamics]`) |
| A13 | 07 §7 | faltam `jk profile check`, `jk profile diff`, `jk board show`, `jk fmt`, `jk class verify --allow-actuation --report` | acrescentar |
| A14 | 20 §3.2 | tipos de media sem árvore (`joker.app.v1+wasm`) | **Resolvido: D13** (tabela única no ref/00 §7.1); as camadas de §2.8.2 |
| A15 | 13 §1 | `conformance/` com "um diretório por caso" | "um ficheiro por suite, `conformance/<área>/<suite>.conformance.toml`" |
| A16 | 14 A13; trilha antiga 6 §7 | o simulador de classe não tinha ficheiro | `classes/<pacote>/<nome>.sim.toml` (§2.4.11) |
| A17 | 14 A8; trilha antiga 7 §4 | `[health] required = [...]` | `health required(…)`, a sintaxe `.jkc` de 17 §3.1 |
| A18 | 05 §11; 15 §2.1 | placas sem versão | `[board] version`; o lock fixa-as |
| A19 | trilha antiga 5 | `gauge` com `keys` | semântica de desvio (§2.4.7), já compatível |
| A20 | trilha antiga 4 | `rate = "128 SPS"`: `SPS` não está no catálogo de 16 §5 | é um rótulo `choice` do perfil `ti/ads1115@1`, e não uma quantidade (§2.2.10); a trilha pode dizê-lo num comentário |
| A21 | 02 §8 | `kind = "modbus.write"` numa ação do manifesto | apagar: quem implementa é o perfil (§2.2.12); 03 §5.2 já não o tem |
| A22 | 04 §6 | `momentary` "mexido com o teclado" sem dizer como | impulso de `pulse` (S11) |
| A23 | 05 §3 | `max_gap` e `max_regs` com omissões só em prosa | tabela de §2.2.6 |
| A24 | 16 §8 | `kind = cycle \| async \| manual \| bridge` | acrescentar `impl` (ref/00 §6 `writer_kind`); `proposal` na 1.1 (**D5**) |
| A25 | 05 §11; 19 §4.1 | placa MCU sem pilha, prioridades de interrupção, áreas da flash, `reset_safe` nem link por omissão | `stack`, `[irq]`, `[areas]`, `reset_safe`, `[link] default` (§2.3.1, §2.3.3), com os nomes que a ref/08 §2.17 pede |
| A26 | esta referência, §2.2.2 (primeira versão) | `joker-link` como driver da família `modbus` | o `joker-link` tem perfis gerados (`joker/link-*`, §2.2.16); um MCU que sirva Modbus usa `modbus-slave` |
