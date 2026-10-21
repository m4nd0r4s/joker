# 16 — Semântica do núcleo: a referência para a Fase 0

Este documento fecha as ambiguidades que bloqueavam a Fase 0 (documento 18, §3). É **normativo** e tem precedência sobre os outros documentos em tudo o que diz respeito a: árbitro, envelope, qualidade, expressões, unidades, aliases, formato do traço, casos de conformidade e registo de códigos. Onde um exemplo noutro documento o contradiga, o exemplo está errado.

A representação comum (versões, nomes, tempo, valores, códigos numéricos, erros) está na `ref/00`, com a mesma precedência (ADR 0023). Este documento fixa a **semântica**; a `ref/00` fixa a **forma**.

---

## 1. Árbitro

| # | Regra |
|---|---|
| A1 | Um **escritor** é o par (origem, nível). A origem é uma app (instância de ciclo e instância async contam como **o mesmo escritor**), a CLI/API de um utilizador, uma ponte, uma implementação ativa ou, numa saída de um MCU, um link (A12). As formas em texto e em binário estão na `ref/00` §5. Uma escrita que chega por uma instância composta (um método da composta, ou um membro pelo caminho aninhado) é do escritor que a fez, e não da composição (P6). |
| A2 | Cada proposta renova o lease do seu escritor: `lease_until = t_tick + lease`. O lease expira quando `t_latch ≥ lease_until`, avaliado no latch do grupo dono. |
| A3 | Qualquer proposta aceite pelo árbitro renova o lease, **seja qual for o resultado no envelope** (`Accepted`, `Clamped`, `Suppressed`, `Rejected(Interlock)`, `Rejected(Quality)`, `Rejected(ClockUnsynced)`…). Só `Rejected(NotOwner)` e `Rejected(WrongType)` não renovam. |
| A4 | Várias propostas do mesmo escritor na mesma saída no mesmo tick: **vence a última**. As propostas async drenadas no passo 2 são anteriores às das tarefas de ciclo do passo 4, e por isso a de ciclo vence. |
| A5 | O valor efetivo é o do nível ativo de maior prioridade (1 = mais alta). Não há empates: dois escritores no mesmo nível da mesma saída é `J0210`. Nos níveis manuais, ocupados em runtime por pessoas, a regra aplica-se pela A13. |
| A6 | Sem nenhum nível ativo, o valor efetivo é `safe`. |
| A7 | **O valor efetivo passa pelo envelope em todos os ticks**, mesmo que ninguém tenha proposto nada de novo. Uma rampa de taxa continua a avançar em direção ao valor efetivo, tick a tick. |
| A8 | O **último valor aplicado** no arranque de uma geração é o `safe`. Uma geração nova que mantém a saída herda o último valor aplicado da geração anterior. |
| A9 | `lease` de cada escritor ≤ `max_lease` da saída (`J0213` se maior). Omissões: `lease` = 3 × período do grupo dono; `max_lease` = 15 min. |
| A10 | Escritas manuais: o `--ttl` é **obrigatório** em cada escrita (CLI, API, HMI). `max_manual` (omissão 8 h) é o limite superior do TTL. |
| A11 | O grupo dono de uma saída é, por ordem: `group` declarado na saída; o grupo da app de maior prioridade que a escreve em ciclo; o grupo implícito `default` (100 ms), para saídas escritas só por async, pontes, CLI ou implementações em async. |
| A12 | **Escritores remotos.** Numa saída de um MCU, quem arbitra e aplica o envelope é o MCU (documento 19, §6); o nó Linux só propõe, pelo Joker Link. Cada saída delega ao link um conjunto de níveis (omissão: os que nenhum escritor local do MCU usa), e cada (link, nível) é um escritor do MCU. A **posse de um nível delegado é do nível**, e não da origem no nó Linux: uma geração nova do nó, que renumera as origens, ocupa o mesmo nível sem `NotOwner`. Uma proposta num nível não delegado é `Rejected(NotOwner)`. O **lease conta no consumidor**: `t_tick` em A2 é o tick do MCU que consome a proposta, e a latência do link não o come. O lease efetivo é `min(lease, max_lease do link)`: o `max_lease` do link (o da sessão) nunca é maior do que o da saída (`J0733`), e é contra ele que o `J0213` verifica o `lease` de um escritor remoto. Uma escrita manual remota guarda o TTL no nó Linux, que a envia com `lease = min(TTL restante, max_lease do link)` e a renova. O MCU **não persiste** os níveis manuais remotos: depois de um reset do MCU, o nível desaparece até o nó Linux o renovar (no máximo um `renew`, `ref/07` §8.1); a persistência da A14 é a do nó Linux. Os resultados voltam por `OUTCOMES`: no nó Linux, o `last_outcome()` de uma tarefa de ciclo sobre uma saída do MCU pode ter um ou dois ticks de atraso. Detalhe: `ref/07` §7.4 e §8.1. |
| A13 | **Ocupação dos níveis manuais.** Um nível manual de uma saída é ocupado por **um** principal. O mesmo principal no mesmo nível renova (substitui valor e TTL). Outro principal no mesmo nível é recusado, com quem o ocupa, até quando e porquê (`J-E1105`), a não ser que peça `takeover`: nesse caso, o *relinquish* do ocupante e a proposta nova entram pela ordem na mesma inbox, e o registo de decisões guarda as duas (`kind = takeover`). Libertar o nível de outro principal exige `force` e fica na auditoria. Detalhe: `ref/09` §5.6. |
| A14 | **Os níveis manuais persistem a reinícios** do `jokerd`. Cada escrita manual aceite grava, com fsync (como os trips), a saída, o nível, o principal, o valor, o motivo e o **prazo em UTC**. No arranque, antes do primeiro tick, um nível com prazo no futuro é reposto; um com prazo passado é descartado (`kind = expiry`). Se o relógio não estiver `synced` no arranque, o UTC não serve para decidir: repõe-se o **TTL restante do último registo** (o que faltava quando foi gravado), contado a partir do arranque em monotónico, e levanta-se o alarme `joker.output.<s>.manual_restored` (gravidade `medium`), para que um operador confirme ou liberte. Uma renovação regrava o registo. Os bilhetes de escrita não sobrevivem ao reinício; os níveis sim. Numa saída de um MCU, o registo é o do nó Linux (A12): no arranque do `jokerd`, o nível reposto volta a ser enviado pelo link com `lease = min(TTL restante, max_lease do link)`; o MCU não o grava. |

## 2. Envelope

| # | Regra |
|---|---|
| E1 | Ordem dos passos: a do documento 01, §7. O primeiro passo que falha decide o resultado. O **passo 0** é o do relógio: numa saída cujo escritor vencedor é uma app com `requires_clock = "synced"`, com o relógio do nó fora de `synced`, a proposta é `Rejected(ClockUnsynced)` antes de qualquer outro passo (tipo, permissivo, gama), a saída vai para `safe` e o lease renova-se (A3), como nas outras rejeições do envelope. |
| E2 | A taxa é medida face ao **último valor aplicado**; no primeiro tick de uma geração nova, face ao `safe` (A8). |
| E3 | `on_interlock = "hold"` mantém o último valor aplicado; `"safe"` (omissão) aplica o `safe`. |
| E4 | `grace` (omissão 0): o `requires_good` só falha depois de a qualidade estar má durante `grace`. Quando falha (passo 3), o resultado é `Rejected(Quality)` e aplica-se o `on_interlock` da saída (E3), como num permissivo que falha: `hold` mantém o último valor aplicado, `safe` aplica o `safe`. Renova o lease (A3). |
| E5 | Um `safe` fora da `range` é `J0203`. |
| E6 | A `range` aceita **expressões** (`range = ["-bess.p_max_discharge", "bess.p_max_charge"]`), avaliadas em cada tick. Um limite com qualidade não `Good` conta como falha do permissivo (passo 2) e aplica `on_interlock`. |
| E7 | Um trip termina com `jk reset` ou com a condição `reset_when` (expressão, na saída), avaliada em cada tick enquanto a saída está em trip. |
| E8 | `Suppressed` (deadband, `min_interval`) não envia nada ao driver, mas renova o lease (A3) e não conta para `max_actuations`. O keepalive (documento 01, §8) nunca é suprimido. |
| E9 | `dedup_window` das ações: omissão 60 s. |
| E10 | Uma proposta com valor **NaN, infinito, ou uma variante de enum inexistente** (e, em geral, um valor que não serve o tipo da saída: `bool` diferente de 0 e 1, `string` com UTF-8 inválido) é `Rejected(WrongType)` no passo 1, e não renova o lease (A3). Não é trap nem erro da tarefa: só essa proposta é recusada (`ref/04` §3.4). É o único caminho, além de uma ligação errada, pelo qual o passo 1 falha em runtime. |

## 3. Qualidade

### 3.1 Acesso pelo SDK

| Chamada | `Good` | `Uncertain` | `Bad` |
|---|---|---|---|
| `get()` | `Ok(v)` | `Err` | `Err` |
| `get_uncertain()` | `Ok(v)` | `Ok(v)` | `Err` |
| `get_or(d)` | `v` | `d` | `d` |
| `sample()` | amostra completa | amostra completa | amostra completa |

`get_or` regista no traço que a leitura foi substituída. Não há contaminação automática da qualidade das saídas da app: quem quiser comunicar isso publica um sinal.

### 3.2 Nas expressões

| Situação | Regra |
|---|---|
| operandos | o resultado tem a pior qualidade dos operandos, com as exceções do curto-circuito |
| `a && b`, `a \|\| b` | curto-circuito **simétrico**: um operando `false (Good)` decide `&&` e um `true (Good)` decide `\|\|`, seja qual for a qualidade do outro e o lado em que está (`false && Bad` e `Bad && false` dão ambos `false (Good)`). Os operandos não têm efeitos, e por isso a ordem não importa. Nos outros casos, a pior qualidade |
| `any(sel)`, `all(sel)` | generalizam o curto-circuito: `any` dá `true (Good)` se algum elemento for `true (Good)`; `all` dá `false (Good)` se algum for `false (Good)`; senão, a pior qualidade dos elementos |
| `count(sel)`, `sum(sel)`, `maxs(sel)`, `mins(sel)` | a pior qualidade dos elementos, sem exceções (um elemento `Bad` torna o resultado `Bad`) |
| `if c then a else b` | `c` `Bad` → resultado `Bad`; `c` `Uncertain` → avalia o ramo e o resultado é, no máximo, `Uncertain` |
| permissivos | só `Good` permite (a não ser `permit_uncertain = true`) |
| alarmes | `Uncertain` avalia normalmente (com a flag no evento); `Bad` → `Unknown` |
| janelas | a qualidade do resultado é a pior qualidade `q` tal que as amostras com qualidade igual ou pior do que `q` cobrem ≥ 10% da duração da janela |
| `held`, `rose`, `fell`, `hyst` | estado inicial `false`; com a entrada `Bad`, o temporizador de `held` recomeça e `rose`/`fell` dão `false` |

### 3.3 Quando uma amostra leva `clock_unsynced`

O estado do relógio do nó **não** altera a qualidade das amostras: vai à parte, no `instant.clock` das apps, no `clock_q` do traço e nas métricas da runtime (documento 01, §19). O motivo `uncertain(clock_unsynced)` só aparece em três casos:

1. o driver reporta-o, porque o **carimbo da origem** (`ts_src`) vem de um relógio remoto que o próprio equipamento diz não estar sincronizado (ref/06 §2.8);
2. um sinal derivado (`calc`) ou publicado por uma app com `requires_clock = "synced"` é produzido enquanto o relógio do nó não está `synced` (o valor depende da hora de parede, como numa tarifa);
3. uma amostra que chega de um MCU cujo `TIME` ainda não foi estabelecido e cujo `ts_src` é a única referência de tempo (ref/07 §6.23).

As escritas de uma app com `requires_clock = "synced"` são outra coisa: são `Rejected(ClockUnsynced)` no passo 0 do envelope, e a saída vai para `safe` (E1; documento 01, §7 e §18).

### 3.4 `debounce` num `bool` lido por poll

Quando o debounce é do núcleo (um driver sem a capacidade `debounce`, ref/06 §3.6, passo 6) e o `bool` é **lido por poll**, e não por flancos:

| # | Regra |
|---|---|
| D1 | O valor publicado só passa de `v` a `¬v` depois de **leituras consecutivas** iguais a `¬v` cujos instantes de aquisição (`ts_acq`) cobrem pelo menos o `debounce`: `ts_acq(última) − ts_acq(primeira) ≥ debounce`. Com um poll de 50 ms e `debounce = "20ms"`, bastam duas leituras seguidas; com `debounce = "100ms"`, três. |
| D2 | Uma leitura igual a `v` pelo meio recomeça a contagem. Uma leitura `Bad` também recomeça, e é publicada como `Bad` (a qualidade nunca espera pelo debounce). |
| D3 | Enquanto espera, o sinal mantém o valor anterior com a qualidade da última leitura, e o `seq` não sobe (não conta como amostra nova, como uma amostra no deadband). |
| D4 | A primeira leitura depois de `connecting` (sem valor anterior) é publicada logo. |
| D5 | O instante da amostra publicada é o `ts_acq` da **primeira** leitura da sequência que confirmou a mudança: é quando o valor mudou, e não quando o debounce acabou. |

Um `debounce` menor do que o `poll` do sinal é aceite e equivale a duas leituras seguidas iguais. Nos flancos (`edges`, com carimbo do kernel), o debounce é o do kernel ou, sem ele, o intervalo mínimo entre flancos, e estas regras não se aplicam.

## 4. Expressões: casos-limite

| Caso | Regra |
|---|---|
| divisão por zero, raiz de negativo, NaN na entrada | resultado `Bad(NaN)` |
| inteiros | a aritmética faz-se em `f64` (um `i64` ou `u64` acima de 2⁵³ perde precisão, e isso é aceite). As **comparações** entre dois inteiros do mesmo sinal (`i64` com `i64`, `u64` com `u64`) são **exatas**, sem passar por `f64`; as outras comparam em `f64` |
| curto-circuito | simétrico (§3.2): `Bad && false` = `false (Good)`, tal como `false && Bad` |
| `°C` (escala afim) | `°C + K → °C`; `°C − °C → K`; `°C − K → °C`; `°C + °C` e `°C * x` são `J0301`; as comparações exigem a mesma dimensão |
| `%` contra `1` | não se misturam: `bms.soc >= 5 %` exige `bms.soc` em `%`; para frações, `0.05` |
| literais sem unidade | só são aceites com operandos adimensionais, como expoentes ou como fatores (`2 * x`); `clamp(x, 0, 1)` exige `x` adimensional |
| durações | `"200ms"` e `"200 ms"` são equivalentes, em todo o lado |
| janelas | máximo 15 min (`J0305` acima); o KPI dos modelos tem outra gramática (documento 06, §10.1) |
| `own(x)` | só é válido dentro de métodos de classe (documento 17); fora deles é `J0306`. É a última proposta do chamador no seu nível (incluindo as anteriores do mesmo tick, 17 §4.1); **sem proposta anterior** (lease expirado ou primeira chamada), devolve o **valor aplicado** de `x` |

Os casos que esta tabela não fecha (tipos, alinhamento de unidades, `J0304` e `J0307`, amostragem das janelas, estado de `held`, `rose`, `fell` e `hyst`) estão na ADR 0034.

## 5. Unidades: catálogo da v1

- **Base:** `s`, `m`, `g` (grama), `A`, `K`, `mol`, `cd`, e `1` (adimensional).
- **Derivadas e de uso comum:** `W`, `Wh`, `J`, `V`, `Ω`, `Hz`, `var`, `varh`, `VA`, `Pa`, `bar`, `N`, `Nm`, `lx`, `°C`, `%`, `rpm`, `l`, `h`, `min`, `d`, `m2`, `m3`, `m/s`, `m/s2`, `m3/h`, `l/min`, `gn` (aceleração da gravidade normal, 9,80665 m/s²).
- **Prefixos** (em qualquer unidade da lista, exceto `°C`, `%`, `1`, `rpm`, `gn`): `µ`, `m`, `k`, `M`, `G`.
- **Compostas:** produtos e quocientes das anteriores (`kW/s`, `%/s`, `m3/h/%`, `°C/min`).
- **Grandezas** (tipos de grandeza que distinguem unidades da mesma dimensão): `active_power`, `reactive_power`, `apparent_power`, `energy`, `reactive_energy`, `temperature`, `temperature_difference`.

Uma unidade fora deste catálogo é `J0109`. Acrescentar unidades é uma mudança de patch no `joker-units`. A sintaxe das compostas, as grandezas das compostas e os códigos dos literais mal formados estão na ADR 0030.

## 6. Caminhos, aliases e instâncias

| # | Regra |
|---|---|
| P1 | Um **alias** é um segundo caminho para o **mesmo** `SignalId`. O registo ordena por caminho canónico (o do dispositivo); os aliases resolvem-se no `jk check`. |
| P2 | Uma saída tem **um só** envelope, declarado por qualquer um dos seus caminhos. Declará-lo por dois é `J0215`. |
| P3 | Instâncias e dispositivos partilham o espaço de nomes do primeiro segmento. Os membros de uma instância são aliases `<instância>.<membro>`. Um nome de instância igual ao de um dispositivo só é permitido para a instância implícita desse mesmo dispositivo; nesse caso, um membro com o mesmo nome de um sinal diferente do dispositivo é `J0604`, e o mesmo vale para os caminhos aninhados de P6. |
| P4 | Os `to` de pipelines, os `calc`, os `alarm` e os `bridge` criam caminhos nos seus prefixos; `pipe:`, `model:` e `class:` são **referências** nas ligações, e não caminhos. |
| P5 | Os nomes de apps, instâncias, dispositivos e grupos são `snake_case` (`[a-z][a-z0-9_]*`). |
| P6 | **Caminhos aninhados.** Um membro de classe (documento 17, §8) cria aliases aninhados: `<instância>.<membro>.<submembro>` para um membro fixo, e `<instância>.<membro>.<elemento>.<submembro>` numa coleção, com `<elemento>` = o **nome da instância** do elemento, e nunca um índice (`station.pumps.p1.running`). São aliases do mesmo `SignalId` (P1): `botao.clicked` e `esp_io.boot.clicked` são o mesmo sinal, e as consultas dos membros também têm caminho aninhado. Uma saída continua a ter um só envelope, declarado por um só dos seus caminhos (P2, `J0215`). Com a profundidade máxima de 3 (`JC0501`), um caminho aninhado tem no máximo 6 segmentos, e cabe sempre nos 8 da `ref/00` §10. Uma instância pode ser membro de várias composições: tem os caminhos de todas, e os seus `out` têm **um só** envelope e **um só** árbitro. As escritas de cada composição chegam como as do escritor que chamou o método ou escreveu pelo caminho (A1), e duas apps no mesmo nível são `J0210`. Quando uma instância com membros `out` chega a apps diferentes por caminhos diferentes, e pelo menos um passa por uma composição, o `jk check` dá o aviso `J0655`, porque a partilha não se vê no `bind` de nenhuma das apps. Numa instância **virtual**, os membros de classe são da implementação: uma app que liga a virtual é escritora só dos `out` da própria classe (os comandos virtuais), lê os membros pelos caminhos aninhados e não escreve neles (`J-E0507` em runtime); por isso, uma virtual de uma classe cujos métodos chamam métodos dos membros é `J0656`. |

## 7. Formato do traço (`.jktrace`) v1

```
ficheiro := cabeçalho [segmento_estado] segmento*
cabeçalho := "JKTR" u16:versão(=1) u32:flags
             [32]u8:hash da geração  [32]u8:hash do registo (lista ordenada de SignalId, tipos, grupos)
             u64:época monotónica  i64:época UTC  u8:qualidade do relógio
segmento := u32:grupo  u32:n_ticks  u32:bytes_comprimidos  zstd(tick*)
tick := u64:seq  u64:t_mono  i64:t_utc  u8:clock_q
        entradas: para cada porta do grupo, em ordem de SignalId: u32:seq  u32:qualidade  u64:valor   (16 B)
        u16:n_propostas   (u32:escritor u32:SignalId u64:valor u16:flags)*        (18 B cada)
        u16:n_resultados  (u32:SignalId u8:resultado u64:valor_aplicado)*          (13 B cada)
        u16:n_saídas_ML   (u32:modelo u32:bytes bytes)*                            (M22)
        u16:n_métodos     (u32:escritor u32:instância u16:método u16:bytes args)*  (documento 17)
segmento_estado := u32:0xFFFF_FFFF  u32:n_apps  u32:bytes_comprimidos  zstd(estado_app*)
estado_app := u8:bytes nome  nome(UTF-8)  u32:grupo  u64:seq do primeiro tick gravado
              u8:estado(0 = exportado, 1 = sem persist, 2 = falhou)  u32:bytes  joker-state (CBOR, ref/04 §5)
```

- Os segmentos são comprimidos com zstd no Linux; no Joker Micro vão sem compressão (bit 0 das `flags` do cabeçalho), e o `jokerd` ou o `jk` comprimem-nos ao recebê-los.
- Todos os inteiros em little-endian. Os `seq` de entrada levam só os 32 bits baixos; o leitor estende-os como no documento 01, §3.2.
- **Escritores:** o `u32` é o número do escritor nesta geração (`ref/00` §5). Os escritores remotos de um MCU (A12) aparecem no traço do MCU como `0x8000_0000 | slot`; o registo de decisões do nó Linux guarda a correspondência slot → (origem, nível) de cada sessão (`ref/07` §8.7). Os números abaixo de `0x8000_0000` são os escritores locais.
- **Traço de um MCU:** o mesmo formato, lido com `jk_trace_read` e transferido pelo `TRACE` do Joker Link. Os instantes vão em **ns** (o MCU multiplica os seus µs por 1000) e no relógio do MCU; os hashes são os da geração e do registo do MCU; `n_saídas_ML` é sempre 0 (`ref/08`).
- **Custo:** 16 B por entrada por tick (e não 12 B), mais propostas e resultados. O documento 03, §7, usa este valor.
- **Segmento de estado** (bit 1 das `flags`). O `jk record` (`source = "record"`, ref/09 §5.16) grava-o logo a seguir ao cabeçalho: para cada app com tarefas de ciclo nos grupos gravados, o blob do `export-state` que já existe para a troca a quente (ref/04 §4.12), pedido à instância de ciclo **entre dois ticks** do grupo dela, com o mesmo prazo de 100 ms, e o `seq` do primeiro tick gravado desse grupo, que é o seguinte. O `jk replay` começa cada app **desse estado** (pelo `migrate`, como numa troca a quente, e com `--app a@versão` pela migração da versão nova) e só depois reproduz os ticks. Uma app sem `persist` (estado 1) ou cujo `export-state` falhou ou passou o prazo (estado 2) começa do `init`, e o replay avisa. Os traços do anel contínuo (`jk trace-dump`, `source = "ring"`), os reconstruídos do historian e os de um MCU **não têm** segmento de estado: o replay começa do `init`, e os primeiros ticks podem divergir do gravado até o estado da app convergir (um integrador, um filtro, uma máquina de estados). O `--diff` marca esses ticks como "antes do estado" e não os conta como divergência.
- **Porta de saída da Fase 0 (determinismo):** dado o mesmo ficheiro, a sequência de `resultados` reproduzida tem de ser **byte a byte** igual à gravada.

## 8. Casos de conformidade

O esquema dos casos é **um só** para o núcleo e para as classes (D10): está na `ref/03` §2.6, e é essa a referência. As regras que valem para os casos do núcleo:

- Um ficheiro por suite, `conformance/<área>/<suite>.conformance.toml`, com `schema = 1` como primeiro par chave/valor, uma `[suite]` (`kind = "core"`, `id`, `spec`, `period`) e um ou mais `[[case]]`.
- A fixture usa a sintaxe do manifesto em listas (`[[signals]]`, `[[outputs]]`, `[[writers]]`, `[[atomic]]`, `[[alarms]]`, `[[calc]]`). Os escritores declaram-se uma vez, com o nível na notação `faixa:prioridade` e `kind` = `cycle` \| `async` \| `manual` \| `bridge` \| `impl` (`proposal` reservado para a 1.1); `manual` exige `ttl`.
- Cada caso é uma linha temporal: ações em `do` e expectativas em `expect`, todas com `at` alinhado com o `period`. Uma proposta `async` entra no passo 2 do tick indicado; uma `cycle`, no passo 4.
- `equals` numa saída é o **valor aplicado** no tick; `outcome` é o resultado da escrita; `level` é o nível ativo (`"none"` sem escritor ativo).
- Os literais escrevem-se na forma canónica em minúsculas (`"good"`, `"clamped_rate"`, `"bad(comm_failure)"`); as formas da CLI (`Clamped(Rate)`) são sinónimos exatos (`ref/00` §2).
- Os mesmos casos correm no Linux e num MCU (Joker Micro), e têm de dar os mesmos bytes (ADR 0020).
- Os valores esperados são **escritos à mão** a partir da especificação, nunca gerados pelo núcleo (a regra do Cravo).

```toml
# conformance/envelope/rate.conformance.toml
schema = 1

[suite]
kind   = "core"
id     = "envelope.rate"
spec   = ["01 §7 passo 5", "16 A7", "16 E2"]
period = "100ms"

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
id = "clamp_then_safe"
do = [{ at = "0ms", propose = "x.p_set", writer = "a", value = "50 kW" }]
expect = [
  { at = "0ms",    output = "x.p_set", outcome = "clamped_rate", equals = "1 kW", level = "optimizer:13" },
  { at = "2200ms", output = "x.p_set", outcome = "none",         equals = "0 kW", level = "none" },
]
```

## 9. Registo de códigos

Todos os códigos vivem num único ficheiro. O registo, com as entradas de todas as referências (incluindo os códigos retirados, que ficam para o número não se reutilizar), está em **`crates/joker-manifest/codes.toml`** (a semente estava em `spec/codes.toml` e passou para lá na tarefa 0b), que é a fonte das contagens (a prosa não as repete; documento 13, §5.1). Cada entrada `[[code]]` tem `id`, `severity` (`error`, `warning`, `error-in-prod`, ou `trap` para os `J-E09xx`), `message`, `doc`, e opcionalmente `explain` (explicação longa), `since` (versão em que o código passa a existir, para as funcionalidades reservadas), `also` (outras referências que o citam), `status = "reserved"` (superfície reservada, ADR 0028) e `retired` com `replaced_by` (um código retirado nunca se reutiliza). **Um código tem uma só gravidade**: quando um caso é aviso e outro é erro, são dois códigos. As mensagens da semente são descrições para a documentação; as mensagens do produto (em inglês, como os identificadores) escrevem-se na tarefa 0b. O `jk explain <código>` lê daí, e um teste de CI falha se um código aparecer na documentação ou no código sem estar no registo (as gamas `A–B` e as listas de números livres não contam).

| Gama | Área |
|---|---|
| `J01xx` | sinais, ligações, unidades, parâmetros |
| `J02xx` | saídas, envelope, árbitro, fallback |
| `J03xx` | expressões, ciclos, RT, barramentos, memória |
| `J04xx` | lock e deriva |
| `J05xx` | ML e pipelines |
| `J06xx` | classes e instâncias |
| `J07xx` | Joker Micro e Joker Link, validação (`J073x`–`J076x` na `ref/07`, `J077x`–`J079x` na `ref/08`) |
| `J08xx` | perfis, placas, simulação, cenários, conformidade, pacotes (`ref/03`); drivers (`ref/06`) |
| `J09xx` | nó, cliente, confiança, segredos (`ref/02`); enrolamento, agente, sessões (`ref/10`) |
| `JCxxxx` / `JWxxxx` | compilador de classes `jkc` (erros e avisos) |
| `J-Exxxx` | erros de runtime vistos pelas apps, pelos SDKs, pela API e pelo Joker Micro |
| `Wxxxx` | avisos com o mesmo número do erro correspondente, em `mode = "dev"` |

As subdivisões destas gamas, uma por referência e sem sobreposições, estão na `ref/00` §8. Cada referência lista os seus códigos; o registo `codes.toml` junta-os todos (documento 22, §8, tarefa 0b).

## 10. Decididos durante a Fase 0 (não bloqueiam o arranque)

| Tema | Onde fica decidido |
|---|---|
| fórmula exata da admissão RT (a margem de 30%, o que entra no WCET) | **decidido:** ADR 0048 |
| `dt()` depois de um `overrun = skip` (período nominal ou real) | ADR na tarefa 8 |
| transições de alarmes a partir de `Unknown` | ADR na tarefa 12 |
| modelo de orçamento de barramentos além do RTU | **decidido:** ADR 0049 |
| esquema JSON do manifesto | **gerado** a partir dos tipos do `joker-manifest` (a `ref/01` é a fonte; o documento 03 é a explicação) |

**Já decidido nesta rodada:** a base dos caminhos de `extends` (relativos ao ficheiro que os declara, `ref/01` §7.1 e §7.5) e a definição exata de "só apertar" (`ref/01` §7.4). Na admissão de tier 2 entra o **máximo** medido, e não o p99,9 (D15, `ref/05`); a fórmula para as apps de tier 1 continua na ADR da tarefa 17.
