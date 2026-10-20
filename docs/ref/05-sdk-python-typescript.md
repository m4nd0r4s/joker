# ref/05 — SDKs de tier 2 (Python, TypeScript) e bibliotecas cliente da API

Referência normativa de duas superfícies:

- **A. SDKs de apps de tier 2**: a API com que se escrevem apps Joker em **TypeScript** (ComponentizeJS/jco) e em **Python** (componentize-py), compiladas para componentes do mundo `joker:runtime`.
- **B. Bibliotecas cliente da API HTTP local**: `joker-client` em **Rust** (a que a `jk` usa), **Python** e **TypeScript**.

Aplica o `ref/00` sem o repetir. A semântica (árbitro, envelope, qualidade, expressões) é a do documento 16, e este documento só a referencia. As decisões que pertencem a outras referências (o mundo WIT e o `joker:meta` da ref/04, os `operationId` da ref/09, o lock da ref/02) estão tomadas nelas e aqui seguem-se; o §32 diz onde ficou cada uma.

---

## 1. Âmbito e fontes

### 1.1 O que cobre

| Parte | Superfície | Pacotes | Estado |
|---|---|---|---|
| A | SDK TypeScript de apps | `@joker/sdk`, `@joker/test`, `@joker/classes-<pacote>` | **v1.0** (Fase 3) |
| A | SDK Python de apps | `joker-sdk`, `joker-testing`, `joker-classes-<pacote>` | **v1.2** (documentos 11 e 21); a API fica fechada aqui |
| B | cliente Rust | crate `joker-client` | **v1.0** (a `jk` é cliente desta crate) |
| B | cliente Python | `joker-client` (PyPI), import `joker_client` | **v1.0** |
| B | cliente TypeScript | `@joker/client` (npm) | **v1.0** (Node ≥ 20); browser na 1.3, com o Studio |

Não cobre: o mundo WIT e o SDK Rust (ref/04), os endpoints e esquemas da API (ref/09), o ficheiro de configuração do cliente e os contextos (ref/02), o formato `.jkc` e o que o `jkc` gera (documento 17).

### 1.2 Fontes

| Documento | O que se tirou |
|---|---|
| 04 §1, §2, §3, §4 | princípios de DX, o mundo WIT, a API Rust de referência, os exemplos e os limites de tier 2 |
| 14 §3, §6, §7 | handles de classe, `Many<T>`, classes locais, implementações ativas, TS e Python |
| 17 §2, §4, §6, §11 | `method` contra `action`, expansão de métodos, o que o `jkc` gera, leitura de `out` |
| 02 §4–§8, §11 | qualidade, tempo, semântica de leitura e escrita, ações, alarmes |
| 16 §1, §3, §9 | árbitro, tabela normativa de acesso à qualidade, registo de códigos |
| 01 §4, §5, §9, §14, §17, §18 | tick, domínio assíncrono, host Wasm, API local, segurança, tempo |
| 07 | comandos da `jk` que os clientes têm de permitir reproduzir |
| 09 ataques 2, 6, 8, 10 | imagem empacotada, duas instâncias, versões, determinismo |
| 20 §2 | local-first: tudo o que o Deck faz, um cliente faz nó a nó |
| 03 §8, §13 | `[apps.*]`, papéis e `api.listen` |
| 11, 21 | o que é v1.0, 1.1 e 1.2 |

### 1.3 Ligações

| Referência | Relação |
|---|---|
| ref/00 | nomes, tempo, valores, qualidade, resultados, erros, limites (aplicados sem exceção) |
| ref/04 | cada função dos SDKs de tier 2 mapeia 1:1 para uma função do mundo `joker:runtime` (§16) e para uma chave do `joker:meta` |
| ref/09 | as bibliotecas cliente seguem os `operationId` e os esquemas do `spec/openapi/joker-api.yaml` (§23) |
| ref/02 | as bibliotecas cliente leem a configuração do cliente `jk` (contextos, identidades) com as mesmas regras da `jk` (§22) |
| 17 | os tipos gerados para as classes (§14) |

---

# Parte A — SDKs de apps de tier 2

## 2. Princípios

1. **Paridade semântica com o SDK Rust, não paridade de sintaxe.** Cada conceito do SDK Rust (ref/04) tem exatamente um equivalente em cada linguagem, com a mesma semântica e o mesmo nome traduzido pelas regras do ref/00, §2. A sintaxe é a idiomática de cada linguagem.
2. **O código é o manifesto da app** (04 §1). As declarações executam-se uma vez, no `jk build`, e produzem a secção `joker:meta`. Não há `app.toml`, e as chaves `[tool.joker]` e `"joker"` só dizem ao `jk build` onde está a app (§18).
3. **Nomes do utilizador em `snake_case` em todas as linguagens** (ref/00 §2): portas, parâmetros, tarefas, registos, membros de classes e variantes de enums. Só a API do SDK segue o estilo da linguagem (`getOr` em TS, `get_or` em Python).
4. **A qualidade nunca desaparece em silêncio** (invariante 4). O equivalente do `?` do Rust é uma **exceção** (`QualityError`) que o SDK apanha na fronteira da tarefa. O equivalente do `panic` é qualquer outra exceção (§9.3).
5. **Ciclo e async são domínios diferentes.** Em TS, são tipos diferentes (`CycleIo<P>` e `TaskIo<P>`), e o compilador apanha os erros. Em Python, são funções diferentes (`def` contra `async def`), o `jk build` apanha os erros comuns por análise estática (`J-E0859`), e o resto dá `J-E0901`/`J-E0802` em runtime.
6. **Mesmo caminho quente do Rust.** Os SDKs de tier 2 usam a imagem empacotada do `on-cycle` (09 ataque 2), e não uma chamada ao host por leitura. Uma chamada ao host a partir de um motor JS ou Python custa microssegundos, e 250 portas custariam milissegundos.
7. **O que se testa é o que vai para produção.** O harness corre o `.wasm` real no wasmtime, com relógio virtual (04 §3.7), e é a mesma crate `joker-test` do Rust, embrulhada (§17).

---

## 3. Pacotes, versões e mundo

| Pacote | Registo | Import | Conteúdo |
|---|---|---|---|
| `@joker/sdk` | npm | `@joker/sdk` | declarações, tipos, runtime do guest, ferramenta de build `joker-build` (Node) |
| `@joker/test` | npm | `@joker/test` | harness com relógio virtual (addon nativo napi-rs sobre `joker-test`), setup para vitest |
| `@joker/classes-<pacote>` | npm | idem | handles gerados pelo `jkc` (`@joker/classes-std`; `acme/water` → `@joker/classes-acme-water`) |
| `joker-sdk` | PyPI | `joker_sdk` | declarações, tipos, runtime do guest, `python -m joker_sdk.build` |
| `joker-testing` | PyPI | `joker_testing` | harness (roda PyO3 sobre `joker-test`), plugin pytest |
| `joker-classes-<pacote>` | PyPI | `joker_classes.<pacote>` | handles gerados (`joker_classes.std`; `acme/water` → `joker_classes.acme.water`); `joker_classes` é um *namespace package* (PEP 420) |

**Porque `joker_sdk` e não `joker`.** O nome `joker` existe no PyPI e é o *namespace* de import de um conjunto de pacotes de terceiros (`joker-stream`, `joker-studio`, …), e `joker-test` também existe (verificado em 2026-09-24). Um `import joker` na documentação levaria as pessoas a `pip install joker` e a instalar outra coisa: é um risco de cadeia de fornecimento, não só de estética. Em Python, o nome de import é o da distribuição com `_`. No npm, `@joker/sdk`, `@joker/client` e `@joker/test` estavam livres na mesma data; a posse da organização `@joker` tem de ser confirmada antes da primeira publicação (§33, A18).

**Versões (ref/00 §1).** Cada SDK tem SemVer próprio e declara o mundo que gera:

| SDK | Mundo gerado | Regra |
|---|---|---|
| `@joker/sdk` 1.y | `joker:runtime@1.0.0` (ou a minor que a 1.y acrescentar) | uma minor do SDK pode passar a gerar a minor seguinte do mundo; o `joker:meta` diz qual (`wit`) |
| `joker-sdk` 1.y | idem | idem |
| 0.x (antes da 1.0) | `joker:runtime@0.x.y` | a política M21 (09 ataque 8): a runtime aceita o mundo atual e a minor anterior |

O `joker:meta` de uma app de tier 2 é o da ref/04 §5 (`spec/cddl/joker-meta.cddl`), com o `sdk-info` (chave 4) preenchido assim: `0` = `lang` (`"typescript"` ou `"python"`), `1` = versão do SDK, `2` = backend (0, `wasm`), `3` = `tier` (2), `4` = `engine` (`"starlingmonkey <versão>"` ou `"cpython <versão>"`), `5` = o pacote (`"@joker/sdk"`, `"joker_sdk"`). O `jk check` usa `tier` e `lang` para aplicar os limites do §19, e a chave 16 (`limits`) leva as sugestões de `memory.max` do §19.

---

## 4. Tipos base

### 4.1 Tipos de sinal em cada linguagem

| Tipo do sinal (ref/00 §6) | Rust | TypeScript | Python | Notas |
|---|---|---|---|---|
| `bool` | `bool` | `boolean` | `bool` | |
| `i64` | `i64` | `bigint` | `int` (marcador `i64` na declaração) | TS usa `bigint` sempre: nunca há perda silenciosa acima de 2⁵³ |
| `u64` | `u64` | `bigint` | `int` (marcador `u64` na declaração) | idem; Python valida a gama ao escrever (`J-E0806`) |
| `f64` | `f64` | `number` | `float` (ou marcador `f64`) | tipo por omissão das grandezas físicas |
| `f32` | `f32` | `number` | `float` com marcador `f32` | promovido a `f64` na leitura (02 §2) |
| `enum<E>` | enum gerado | união de literais com os nomes das variantes (`"run"`) | subclasse de `joker_sdk.Enum` (`IntEnum`) | TS usa o nome, como o JSON (ref/00 §4.1); Python usa o membro (`Mode.run`) |
| `bits<N>` | tipo gerado | `Bits<Nomes>` (`.word: bigint`, `.has(nome)`) | subclasse de `joker_sdk.Bits` (`IntFlag`) | |
| `string` | `String` (≤ 64 B) | `string` | `str` | validado em UTF-8 ≤ 64 B ao escrever (`J-E0805`) |
| `bytes` | `Vec<u8>` (≤ 64 B) | `Uint8Array` | `bytes` | idem |
| registo (eventos, argumentos, `Internal`) | `#[derive(Record)]` | `record({...})` → `Infer<typeof R>` | classe com `@record` | CBOR determinístico, mapa com **chaves inteiras** = posição do campo a contar de 0 (D2, ref/00 §4.1, ref/04 §4.10); os nomes só no `joker:meta` |

Os números nas portas estão **sempre na unidade declarada pela porta**. A runtime converte da unidade do sinal (02 §3). As APIs dos SDKs não aceitam literais de quantidade (`"50 kW"`): isso é das interfaces de texto (ref/00 §4.1), como a biblioteca cliente.

### 4.2 Tempo

| Conceito | TypeScript | Python |
|---|---|---|
| duração | `Duration` (`.ns: bigint`, `.seconds: number`); as funções aceitam `DurationLike = string \| Duration` | `Duration` (`.ns: int`, `.seconds: float`); as funções aceitam `DurationLike = str \| Duration \| datetime.timedelta` |
| instante | `Instant` (`.monoNs: bigint`, `.utcNs: bigint`, `.clock: ClockQuality`) | `Instant` (`.mono_ns: int`, `.utc_ns: int`, `.clock: ClockQuality`) |
| aritmética | `now.minus("24h")`, `now.plus(d)`, `a.since(b)` | `now - "24h"`, `now + d`, `a - b` → `Duration` |
| UTC | `now.utcSeconds(): bigint`, `now.toDate(): Date` (ms) | `now.utc_seconds() -> UtcSeconds`, `now.datetime() -> datetime` (µs, `tz=UTC`) |

As strings de duração seguem o ref/00, §3.1. Um literal inválido é um `TypeError` na declaração (erro no `jk build`) ou `J-E0806` em runtime. Os instantes em nanossegundos são `bigint` em TS porque o monotónico em ns passa 2⁵³ ao fim de ~104 dias de funcionamento, e o UTC em ns já o passou há muito.

### 4.3 Qualidade e amostras

```ts
export type QualityCode = "good" | "uncertain" | "bad";
export type QualityReason =
  | "none" | "overridden" | "local_override" | "stale" | "not_confirmed" | "substituted"
  | "out_of_distribution" | "sensor_cal" | "clock_unsynced" | "initial" | "frozen" | "partial_write"
  | "comm_failure" | "device_fault" | "out_of_range" | "config_error" | "not_connected" | "nan"
  | "tripped" | "deadline" | "unknown";            // "unknown": motivo futuro (ref/00 §1, regra 2)
export interface Quality { readonly code: QualityCode; readonly reason: QualityReason; }
```

```python
class QualityCode(StrEnum):
    GOOD = "good"; UNCERTAIN = "uncertain"; BAD = "bad"

class QualityReason(StrEnum):
    NONE = "none"; OVERRIDDEN = "overridden"; LOCAL_OVERRIDE = "local_override"
    STALE = "stale"; NOT_CONFIRMED = "not_confirmed"; SUBSTITUTED = "substituted"
    OUT_OF_DISTRIBUTION = "out_of_distribution"; SENSOR_CAL = "sensor_cal"
    CLOCK_UNSYNCED = "clock_unsynced"; INITIAL = "initial"; FROZEN = "frozen"
    PARTIAL_WRITE = "partial_write"; COMM_FAILURE = "comm_failure"; DEVICE_FAULT = "device_fault"
    OUT_OF_RANGE = "out_of_range"; CONFIG_ERROR = "config_error"; NOT_CONNECTED = "not_connected"
    NAN = "nan"; TRIPPED = "tripped"; DEADLINE = "deadline"; UNKNOWN = "unknown"

@dataclass(frozen=True, slots=True)
class Quality:
    code: QualityCode
    reason: QualityReason
    @staticmethod
    def good() -> Quality: ...
    @staticmethod
    def uncertain(reason: QualityReason | str) -> Quality: ...
    @staticmethod
    def bad(reason: QualityReason | str) -> Quality: ...
```

Os valores das enums são as strings do ref/00, §4.3, que são também as do JSON da API: um `Quality` de uma app e um `Quality` lido pelo cliente têm a mesma forma. Um motivo numérico entre 20 e 255 chega ao SDK como `unknown` (motivos futuros e privados, ref/00 §4.3).

`Sample<T>` / `Sample[T]` (campos em `snake_case` em Python e em `camelCase` em TS, porque são API do SDK):

| Campo | Tipo | Significado (02 §5) |
|---|---|---|
| `value` | `T` | o valor entregue pelo host. Existe sempre; com qualidade `bad` não tem significado de processo e só serve para diagnóstico |
| `quality` | `Quality` | |
| `ts_src` / `tsSrc` | `int \| None` / `bigint \| null` | ns UTC do dispositivo remoto |
| `ts_hw` / `tsHw` | idem | ns monotónicos da captura em hardware |
| `ts_acq` / `tsAcq` | `int` / `bigint` | ns monotónicos da receção |
| `seq` | `int` / `bigint` | contador por sinal |

E os mesmos acessores de uma porta: `get()`, `get_uncertain()` / `getUncertain()`, `get_or(d)` / `getOr(d)`, `is_good()` / `isGood()`.

### 4.4 Resultados de escrita

A forma é a do JSON do ref/00, §4.4, nas duas linguagens:

```ts
export type RejectReason =
  | "wrong_type" | "interlock" | "quality" | "range" | "atomic_group" | "not_owner"
  | "clock_unsynced" | "budget" | "unknown";
export type WriteOutcome =
  | { readonly outcome: "accepted" }
  | { readonly outcome: "clamped_range"; readonly applied: number | bigint }
  | { readonly outcome: "clamped_rate"; readonly applied: number | bigint }
  | { readonly outcome: "suppressed" }
  | { readonly outcome: "overridden"; readonly level: string }      // "manual:3" (ref/00 §5)
  | { readonly outcome: "deferred" }
  | { readonly outcome: "shadowed" }
  | { readonly outcome: "rejected"; readonly reason: RejectReason }
  | { readonly outcome: "tripped" }
  | { readonly outcome: "unknown" };
```

```python
class Outcome(StrEnum):
    ACCEPTED = "accepted"; CLAMPED_RANGE = "clamped_range"; CLAMPED_RATE = "clamped_rate"
    SUPPRESSED = "suppressed"; OVERRIDDEN = "overridden"; DEFERRED = "deferred"
    SHADOWED = "shadowed"; REJECTED = "rejected"; TRIPPED = "tripped"; UNKNOWN = "unknown"

class RejectReason(StrEnum):
    WRONG_TYPE = "wrong_type"; INTERLOCK = "interlock"; QUALITY = "quality"; RANGE = "range"
    ATOMIC_GROUP = "atomic_group"; NOT_OWNER = "not_owner"; CLOCK_UNSYNCED = "clock_unsynced"
    BUDGET = "budget"; UNKNOWN = "unknown"

@dataclass(frozen=True, slots=True)
class WriteOutcome:
    outcome: Outcome
    applied: int | float | None = None      # só em clamped_range / clamped_rate
    level: str | None = None                # só em overridden, "<faixa>:<nível>"
    reason: RejectReason | None = None      # só em rejected
    @property
    def ok(self) -> bool: ...               # accepted, clamped_*, suppressed
```

- `none` (valor 0 do ref/00 §4.4, "sem proposta neste tick") não é um `WriteOutcome`: `last_outcome()` devolve `None`/`null`.
- O WIT distingue `clamped(clamp)` (ref/00 §4.4); o SDK traduz para `clamped_range` e `clamped_rate`.
- `overridden(u8)` chega com o nível absoluto; o SDK formata-o como `"<faixa>:<nível>"` com as faixas do documento 01, §6.
- Uma variante nova do WIT chega como `unknown` (ref/00 §1, regra 2).
- Em Python, `match` funciona sobre o `dataclass`: `case WriteOutcome(outcome=Outcome.CLAMPED_RATE, applied=v)`.

### 4.5 Erros

```ts
export declare class JokerError extends Error { readonly code: string; }       // "J-E0800"
export declare class QualityError extends JokerError { readonly port: string; readonly sample: Sample<unknown>; }
export declare class DomainError extends JokerError {}          // J-E0901, J-E0802
export declare class ActionError extends JokerError {
  readonly kind: "denied" | "forbidden" | "timeout" | "device" | "invalid" | "busy" | "not_connected"
               | "cancelled" | "conflict" | "unsupported" | "failed" | "other";   // ActionErrorKind da ref/04 §4.7.1
  readonly detail: string;
}
export declare class CapabilityError extends JokerError { readonly capability: string; }
export declare class LimitError extends JokerError {}           // J-E0804, J-E0805, J-E0806, J-E0811
export declare class UnavailableError extends JokerError {}     // J-E0601 (item @unstable, stub em dev) ou J-E0809 (API de um mundo posterior ao gerado)
export declare class KvError extends JokerError {}
export declare class HostError extends JokerError {}            // erro do host com código próprio (ref/04)
```

Python tem a mesma hierarquia (`class JokerError(Exception)` com `code: str` e `message: str`, e as subclasses com os mesmos nomes e campos em `snake_case`). O `str()` de um `JokerError` começa sempre pelo código (`"J-E0800: port 'grid_power' is bad(comm_failure)"`), para que `jk explain` funcione a partir de qualquer log (ref/00 §8).

### 4.6 Declarações auxiliares

```python
class ClockQuality(StrEnum):
    SYNCED = "synced"; HOLDOVER = "holdover"; UNSYNCED = "unsynced"

class Duration:
    ns: int
    def __init__(self, value: DurationLike) -> None: ...
    @property
    def seconds(self) -> float: ...
    def __add__(self, other: DurationLike) -> Duration: ...

class Instant:
    mono_ns: int
    utc_ns: int
    clock: ClockQuality
    def utc_seconds(self) -> int: ...
    def datetime(self) -> datetime: ...
    def __add__(self, d: DurationLike) -> Instant: ...
    @overload
    def __sub__(self, other: Instant) -> Duration: ...
    @overload
    def __sub__(self, other: DurationLike) -> Instant: ...

@dataclass(frozen=True, slots=True)
class Sample(Generic[T]):
    value: T
    quality: Quality
    ts_src: int | None
    ts_hw: int | None
    ts_acq: int
    seq: int
    def get(self) -> T: ...
    def get_uncertain(self) -> T: ...
    def get_or(self, default: T) -> T: ...
    def is_good(self) -> bool: ...

@dataclass(frozen=True, slots=True)
class Change:
    port: str
    sample: Sample[Any]
```

```ts
export type ClockQuality = "synced" | "holdover" | "unsynced";
export declare class Duration {
  static parse(d: DurationLike): Duration;
  readonly ns: bigint;
  get seconds(): number;
  plus(d: DurationLike): Duration;
}
export declare class Instant {
  readonly monoNs: bigint; readonly utcNs: bigint; readonly clock: ClockQuality;
  utcSeconds(): bigint; toDate(): Date;
  plus(d: DurationLike): Instant; minus(d: DurationLike): Instant; since(other: Instant): Duration;
}
export interface Sample<T> {
  readonly value: T; readonly quality: Quality;
  readonly tsSrc: bigint | null; readonly tsHw: bigint | null; readonly tsAcq: bigint; readonly seq: bigint;
  get(): T; getUncertain(): T; getOr(d: T): T; isGood(): boolean;
}
export interface Bits<B extends string> { readonly word: bigint; has(bit: B): boolean; active(): B[] }
export interface Readable<T> { sample(): Sample<T> }
export interface Secrets { get(name: string): Promise<SecretString> }
export interface SecretString { reveal(): string; toString(): string; toJSON(): string }   // "***"
// opacos: o SDK constrói-os e quem escreve apps não os usa diretamente
export interface SecretDecl { readonly kind: "secret"; readonly name: string }
export interface ModelDecl { readonly kind: "model" }                                     // v1.1
export interface ImplementsDecl<C> { readonly kind: "implements"; readonly __c?: C }
export interface LocalClassDef { inputs?: object; outputs?: object; params?: object }
export interface CycleDecl<P, S> { readonly kind: "cycle"; readonly __p?: P; readonly __s?: S }
export interface TaskDecl<P> { readonly kind: "task"; readonly __p?: P }
export interface AppComponent { readonly name: string; readonly version: string }
export interface ActionCx {
  readonly key: string; readonly caller: string; readonly deadline: bigint;     // deadline: mono ns
  progress(fraction: number | null, message?: string): void; isCancelled(): boolean;
}
// CycleView<Cy, Req> e TaskView<Ta, Req>: a vista gerada de uma classe, com os membros das capacidades
// de Req obrigatórios (§14.1); LocalCycle/LocalTask: o mesmo para classes locais
```

---

## 5. Declarar uma app

### 5.1 A escolha em cada linguagem

| | Rust (ref/04) | TypeScript | Python |
|---|---|---|---|
| app | `#[joker::app(name, version)] struct` | `export default app({ name, version, … })` (**objeto de definição**) | `@app(name=…, version=…)` sobre `class X(App)` |
| portas | campos com atributos | propriedades de `inputs`, `outputs`, `params`, … | atributos de classe (descritores) |
| tarefa de ciclo | `#[joker::cycle] fn(io, st)` | `cycles: { nome: cycle(opts, (io, st) => …) }` | `@cycle(…) def nome(self, st)` |
| tarefa async | `#[joker::task] async fn(io)` | `tasks: { nome: task.every(…, async (io) => …) }` | `@task.every(…) async def nome(self)` |
| vista de I/O | `CycleIo<A>` / `TaskIo<A>` | `CycleIo<P>` / `TaskIo<P>` (tipos inferidos da definição) | `self`, que é a vista do domínio onde a instância Wasm corre |

**TypeScript: objeto de definição, e não decoradores.** Com um objeto, o TypeScript infere o tipo de `io` a partir da própria declaração: `io.grid_power.get()` é `number`, `io.battery.set("x")` é um erro de tipos, e `io.grid_power.history()` só existe em `TaskIo`. Com decoradores (TC39, TS ≥ 5.0), os campos de uma classe não conseguem mudar o tipo de `this` por domínio, as tarefas de ciclo e async partilhariam `this` (que são duas instâncias diferentes, 09 ataque 6), e a inferência exigiria gerar código. O objeto funciona em JavaScript sem compilador, e é o que o documento 04 já mostrava.

**Python: classe com decoradores, e `self` como vista.** É a forma idiomática, e os descritores dão tipos a `self.grid_power.get()` no mypy e no pyright sem geração de código. `self` **não** é estado: uma atribuição `self.x = …` levanta `J-E0803`, porque o ciclo e o async correm em instâncias Wasm diferentes e um atributo escrito numa nunca seria visto na outra. O estado do ciclo vai para `st` (§9.1), e o que se partilha vai por `Internal`, `Mailbox` ou eventos (§10). O decorador `@app` fixa a identidade e valida; a classe base `App` dá os tipos da vista (`now()`, `dt()`, `kv`, …). Uma classe com `@app` que não herde de `App` é `J-E0850`.

### 5.2 Python: API de declaração

Excerto normativo do `joker_sdk/__init__.pyi`. Todos os argumentos depois do tipo são *keyword-only*.

```python
from __future__ import annotations
from collections.abc import AsyncIterator, Awaitable, Callable, Iterator, Mapping, Sequence
from datetime import timedelta
from typing import Annotated, Any, ClassVar, Generic, Literal, Protocol, Self, TypeVar, overload

T = TypeVar("T"); R = TypeVar("R"); A = TypeVar("A"); S = TypeVar("S")
H = TypeVar("H", bound="ClassHandle")
DurationLike = str | "Duration" | timedelta
Severity = Literal["low", "medium", "high", "critical"]

# ---- marcadores de tipo (ref/00 §6): só descrevem o sinal; os handles usam int e float
i64 = Annotated[int, "i64"]                    # `int` sozinho = i64
u64 = Annotated[int, "u64"]
f64 = Annotated[float, "f64"]                  # `float` sozinho = f64
f32 = Annotated[float, "f32"]
UtcSeconds = Annotated[int, "utc_seconds"]
class Enum(IntEnum): ...                       # enums do utilizador: class Mode(Enum): off = 0; run = 2
class Bits(IntFlag): ...                       # palavras de bits: class Status(Bits): alarm_overtemp = 1 << 3

# ---- registos (eventos, argumentos e resultados de ações, Internal, Mailbox)
def record(cls: type[T]) -> type[T]: ...       # dataclass congelada; campos com anotações de tipo
def field(*, unit: str | None = None, default: Any = ..., default_factory: Callable[[], Any] | None = None) -> Any: ...
def state(cls: type[S] | None = None, *, persist: bool = False) -> Any: ...   # estado de ciclo (§9.1)

# ---- a app
def app(*, name: str, version: str | None = None, description: str | None = None) -> Callable[[type[T]], type[T]]: ...

class App:
    # vista comum aos dois domínios
    def now(self) -> Instant: ...
    # só ciclo
    def dt(self) -> Duration: ...
    # só async
    def snapshot(self, *ports: "Readable[Any]") -> tuple[Sample[Any], ...]: ...
    def subscribe(self, *ports: "Readable[Any]") -> AsyncIterator[Change]: ...
    async def sleep(self, d: DurationLike) -> None: ...
    async def sleep_until(self, t: Instant) -> None: ...
    kv: ClassVar["Kv"]
    secrets: ClassVar["Secrets"]

# ---- portas (camada 1)
class Input(Generic[T]):
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 optional: bool = False, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> InputHandle[T]: ...

class InputMany(Generic[T]):
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 min: int = 0, max: int = 1024, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> ManyPorts[InputHandle[T]]: ...

class Output(Generic[T]):
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 optional: bool = False, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> OutputHandle[T]: ...

class OutputMany(Generic[T]):
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 min: int = 0, max: int = 1024, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> ManyPorts[OutputHandle[T]]: ...

class Param(Generic[T]):
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 range: tuple[float, float] | None = None, default: T | None = None,
                 doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> ParamHandle[T]: ...

class Internal(Generic[R]):
    def __init__(self, type: type[R], *, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> InternalHandle[R]: ...

class Mailbox(Generic[R]):
    def __init__(self, type: type[R], *, capacity: int, to: Literal["cycle", "async"],
                 doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> MailboxHandle[R]: ...

class EventInput(Generic[R]):
    def __init__(self, type: type[R], *, optional: bool = False, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> EventInputHandle[R]: ...

class EventOutput(Generic[R]):
    def __init__(self, type: type[R], *, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> EventOutputHandle[R]: ...

class Action(Generic[A, R]):                     # porta ligada a uma ação de um dispositivo (02 §8)
    def __init__(self, args: type[A] | None = None, result: type[R] | None = None, *,
                 optional: bool = False, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> ActionHandle[A, R]: ...

class Alarm:
    def __init__(self, *, severity: Severity, message: str, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> AlarmHandle: ...

class Published(Generic[T]):                     # sinal app.<app>.<nome> (04 §3.1)
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> PublishedHandle[T]: ...

class Counter:                                   # contador monotónico publicado (u64)
    def __init__(self, *, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> CounterHandle: ...

class Secret:                                    # segredo declarado (ref/04 §2.5.2; joker:meta chave 13)
    def __init__(self, name: str, *, optional: bool = False, doc: str | None = None) -> None: ...

class Model:                                     # v1.1 (ML); reservado
    def __init__(self, *, inputs: str, outputs: str, doc: str | None = None) -> None: ...

# ---- classes (camada 2, §14)
def device(cls: type[H], *, requires: Sequence[str] = (), optional: bool = False) -> H: ...
def devices(cls: type[H], min: int, max: int, *, requires: Sequence[str] = ()) -> Many[H]: ...
def implements(cls: type[ClassHandle], *, capabilities: Sequence[str] = ()) -> Callable[[type[T]], type[T]]: ...
def local_class(cls: type[T]) -> type[T]: ...

# ---- tarefas (§9)
def cycle(period: str, *, wcet: str | None = None, state: type[S] | None = None,
          init: Callable[[Any], S] | None = None, optional: bool = False) -> Callable[[Callable[..., None]], Any]: ...

class task:
    @staticmethod
    def every(period: str, *, jitter: str | None = None) -> Callable[[Callable[..., Awaitable[None]]], Any]: ...
    @staticmethod
    def at(cron: str) -> Callable[[Callable[..., Awaitable[None]]], Any]: ...
    @staticmethod
    def on(port: str) -> Callable[[Callable[..., Awaitable[None]]], Any]: ...        # handler(self, s: Sample[T])
    @staticmethod
    def on_event(port: str) -> Callable[[Callable[..., Awaitable[None]]], Any]: ...  # handler(self, e: R)
    @staticmethod
    def startup() -> Callable[[Callable[..., Awaitable[None]]], Any]: ...

def on_action(name: str) -> Callable[[Callable[..., Awaitable[Any]]], Any]: ...     # só com @implements
def on_params(fn: Callable[[Any, Mapping[str, Any]], None]) -> Any: ...
def migrate(fn: Callable[[Any, Any, str], Any]) -> Any: ...                         # (self, previous, from_version) -> state
```

### 5.3 TypeScript: API de declaração

Excerto normativo do `@joker/sdk/index.d.ts`. `P` é o tipo das portas inferido da definição.

```ts
export type DurationLiteral = `${number}${"ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"}`
                            | `${number} ${"ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"}`;
export type DurationLike = DurationLiteral | Duration;
export type Severity = "low" | "medium" | "high" | "critical";

// ---- esquemas de valores
export interface PortOpts { unit?: string; quantity?: string; optional?: boolean; doc?: string }
export interface ManyOpts extends Omit<PortOpts, "optional"> { min?: number; max?: number }
export interface ParamOpts<T> extends Omit<PortOpts, "optional"> { default?: T; range?: readonly [number, number] }
export interface EnumSchema<V extends string> { readonly variants: Readonly<Record<V, number>> }
export interface BitsSchema<B extends string> { readonly bits: Readonly<Record<B, number>> }
export declare function enumType<V extends string>(variants: Record<V, number>): EnumSchema<V>;
export declare function bitsType<B extends string>(bits: Record<B, number>): BitsSchema<B>;

export interface FieldSchema<T> { readonly __t?: T }
export declare const t: {
  bool(): FieldSchema<boolean>; i64(o?: { unit?: string }): FieldSchema<bigint>; u64(o?: { unit?: string }): FieldSchema<bigint>;
  f64(o?: { unit?: string }): FieldSchema<number>; f32(o?: { unit?: string }): FieldSchema<number>;
  string(o?: { maxBytes?: number }): FieldSchema<string>; bytes(o?: { maxBytes?: number }): FieldSchema<Uint8Array>;
  utcSeconds(): FieldSchema<bigint>; enum<V extends string>(e: EnumSchema<V>): FieldSchema<V>;
  list<T>(item: FieldSchema<T>, maxLen: number): FieldSchema<readonly T[]>;
  record<R>(r: RecordSchema<R>): FieldSchema<R>; optional<T>(f: FieldSchema<T>): FieldSchema<T | null>;
};
export interface RecordSchema<R> { readonly fields: { [K in keyof R]: FieldSchema<R[K]> } }
export declare function record<F extends Record<string, FieldSchema<any>>>(fields: F): RecordSchema<{ [K in keyof F]: F[K] extends FieldSchema<infer T> ? T : never }>;
export type Infer<S> = S extends RecordSchema<infer R> ? R : never;

// ---- declarações de portas (camada 1)
export interface InputDecl<T> { readonly kind: "input"; readonly __t?: T }
export interface InputManyDecl<T> { readonly kind: "input_many"; readonly __t?: T }
export interface OutputDecl<T> { readonly kind: "output"; readonly __t?: T }
export interface OutputManyDecl<T> { readonly kind: "output_many"; readonly __t?: T }
export interface ParamDecl<T> { readonly kind: "param"; readonly __t?: T }
export interface InternalDecl<R> { readonly kind: "internal"; readonly __t?: R }
export interface MailboxDecl<R, D extends "cycle" | "async"> { readonly kind: "mailbox"; readonly to: D; readonly __t?: R }
export interface EventInputDecl<R> { readonly kind: "event_input"; readonly __t?: R }
export interface EventOutputDecl<R> { readonly kind: "event_output"; readonly __t?: R }
export interface ActionDecl<A, R> { readonly kind: "action"; readonly __a?: A; readonly __r?: R }
export interface AlarmDecl { readonly kind: "alarm" }
export interface PublishedDecl<T> { readonly kind: "published"; readonly __t?: T }
export interface CounterDecl { readonly kind: "counter" }

interface ScalarCtors<D, O> {
  bool(o?: O): D & { __t?: boolean };
  i64(o?: O | string): D & { __t?: bigint };
  u64(o?: O | string): D & { __t?: bigint };
  f64(o?: O | string): D & { __t?: number };                 // input.f64("kW") ≡ input.f64({ unit: "kW" })
  f32(o?: O | string): D & { __t?: number };
  enum<V extends string>(e: EnumSchema<V>, o?: O): D & { __t?: V };
  bits<B extends string>(b: BitsSchema<B>, o?: O): D & { __t?: Bits<B> };
  string(o?: O): D & { __t?: string };
  bytes(o?: O): D & { __t?: Uint8Array };
}
export declare const input: ScalarCtors<InputDecl<unknown>, PortOpts> & {
  event<R>(r: RecordSchema<R>, o?: { optional?: boolean; doc?: string }): EventInputDecl<R>;
};
export declare const inputMany: ScalarCtors<InputManyDecl<unknown>, ManyOpts>;
export declare const output: ScalarCtors<OutputDecl<unknown>, PortOpts> & {
  event<R>(r: RecordSchema<R>, o?: { doc?: string }): EventOutputDecl<R>;
};
export declare const outputMany: ScalarCtors<OutputManyDecl<unknown>, ManyOpts>;
export declare const param: {
  bool(o?: ParamOpts<boolean>): ParamDecl<boolean>; i64(o?: ParamOpts<bigint>): ParamDecl<bigint>;
  u64(o?: ParamOpts<bigint>): ParamDecl<bigint>; f64(o?: ParamOpts<number>): ParamDecl<number>;
  f32(o?: ParamOpts<number>): ParamDecl<number>; string(o?: ParamOpts<string>): ParamDecl<string>;
  enum<V extends string>(e: EnumSchema<V>, o?: ParamOpts<V>): ParamDecl<V>;
};
export declare function internal<R>(r: RecordSchema<R>, o?: { doc?: string }): InternalDecl<R>;
export declare function mailbox<R, D extends "cycle" | "async">(r: RecordSchema<R>, o: { capacity: number; to: D; doc?: string }): MailboxDecl<R, D>;
export declare function action<A = void, R = void>(o?: { args?: RecordSchema<A>; result?: RecordSchema<R>; optional?: boolean; doc?: string }): ActionDecl<A, R>;
export declare function alarm(o: { severity: Severity; message: string; doc?: string }): AlarmDecl;
export declare function published<T>(d: OutputDecl<T>): PublishedDecl<T>;        // published(output.f64("kW"))
export declare function counter(o?: { doc?: string }): CounterDecl;
export declare function secret(name: string, o?: { optional?: boolean }): SecretDecl;   // ref/04 §2.5.2
export declare function model(o: { inputs: string; outputs: string }): ModelDecl;  // v1.1, reservado

// ---- classes (camada 2, §14)
export interface ClassSchema<Cy, Ta, Caps extends string> { readonly class: string; readonly __cy?: Cy; readonly __ta?: Ta; readonly __caps?: Caps }
export interface DeviceDecl<C, Req extends string> { readonly kind: "device"; readonly __c?: C; readonly __r?: Req }
export interface ManyDecl<C, Req extends string> { readonly kind: "many"; readonly __c?: C; readonly __r?: Req }
export declare function device<Cy, Ta, Caps extends string, Req extends Caps = never>(c: ClassSchema<Cy, Ta, Caps>, o?: { requires?: readonly Req[]; optional?: boolean }): DeviceDecl<ClassSchema<Cy, Ta, Caps>, Req>;
export declare function many<Cy, Ta, Caps extends string, Req extends Caps = never>(c: ClassSchema<Cy, Ta, Caps>, o: { min: number; max: number; requires?: readonly Req[] }): ManyDecl<ClassSchema<Cy, Ta, Caps>, Req>;
export declare function implementsClass<Cy, Ta, Caps extends string>(c: ClassSchema<Cy, Ta, Caps>, o?: { capabilities?: readonly Caps[] }): ImplementsDecl<ClassSchema<Cy, Ta, Caps>>;
export declare function localClass<L extends LocalClassDef>(def: L): ClassSchema<LocalCycle<L>, LocalTask<L>, never>;

// ---- tarefas (§9)
export interface CycleOpts<P, S> { period: DurationLiteral; wcet?: DurationLiteral; state?: (io: CycleIo<P>) => S; persist?: boolean; optional?: boolean }
export declare function cycle<P, S = void>(opts: CycleOpts<P, S> | DurationLiteral, body: (io: CycleIo<P>, st: S) => void): CycleDecl<P, S>;
export declare const task: {
  every<P>(period: DurationLiteral, body: (io: TaskIo<P>) => Promise<void>): TaskDecl<P>;
  every<P>(period: DurationLiteral, opts: { jitter?: DurationLiteral }, body: (io: TaskIo<P>) => Promise<void>): TaskDecl<P>;
  at<P>(cron: string, body: (io: TaskIo<P>) => Promise<void>): TaskDecl<P>;
  on<P, K extends keyof P & string>(port: K, body: (io: TaskIo<P>, s: Sample<ValueOf<P[K]>>) => Promise<void>): TaskDecl<P>;
  onEvent<P, K extends keyof P & string>(port: K, body: (io: TaskIo<P>, e: RecordOf<P[K]>) => Promise<void>): TaskDecl<P>;
  startup<P>(body: (io: TaskIo<P>) => Promise<void>): TaskDecl<P>;
};

// ---- a app
export interface AppDef<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta> {
  name: string; version?: string; description?: string;
  inputs?: I; outputs?: O; params?: Pa; internals?: In; mailboxes?: Mb; alarms?: Al;
  published?: Pu; devices?: Dv; actions?: Ac; secrets?: readonly SecretDecl[];
  models?: Readonly<Record<string, ModelDecl>>;                             // v1.1
  implements?: ImplementsDecl<any>;
  cycles?: Cy; tasks?: Ta;
  onAction?: Record<string, (io: TaskIo<any>, args: any, cx: ActionCx) => Promise<any>>;   // só com implements
  onParams?: (params: Readonly<Record<string, unknown>>) => void;
  migrate?: (previous: unknown, fromVersion: string) => unknown;
}
export declare function app<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta>(def: AppDef<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta>): AppComponent;
```

As tarefas recebem `io` com o tipo `CycleIo<P>` ou `TaskIo<P>`, em que `P` junta todas as portas da definição (as chaves de `inputs`, `outputs`, `params`, `devices`, … vivem num só espaço de nomes, e uma chave repetida em dois grupos é `J-E0857`). Em TS as tarefas escrevem-se dentro de `app({...})` para que a inferência funcione; uma tarefa definida fora tem de anotar `io` com `CycleIo<typeof ports>`, e o SDK exporta `defineCycle(ports, …)` e `defineTask(ports, …)` para isso.

### 5.4 Tabela de declarações

A última coluna diz onde a declaração fica no `joker:meta` (ref/04 §5; as chaves são inteiras, e `port.1` é o `port-kind`).

| Conceito | Rust (ref/04) | TypeScript | Python | `joker:meta` |
|---|---|---|---|---|
| app | `#[joker::app(name, version)]` | `app({ name, version })` | `@app(name=, version=)` | chaves 1 e 2 |
| entrada | `#[input(unit, quantity)] x: Input<f64>` | `inputs: { x: input.f64({ unit, quantity }) }` | `x = Input(float, unit=, quantity=)` | porta, `1: 0` (input) |
| entrada múltipla | `#[input(many)] InputMany<T>` | `inputs: { x: inputMany.f64(...) }` | `InputMany(float, ...)` | porta com `6` (`many`) |
| saída | `#[output] Output<T>` | `outputs: { x: output.f64(...) }` | `Output(float, ...)` | porta, `1: 1` (output) |
| saída múltipla | `#[output(many)] OutputMany<T>` | `outputMany.f64(...)` | `OutputMany(float, ...)` | idem, com `6` |
| parâmetro | `#[param(unit, range, default)] Param<T>` | `params: { x: param.f64({ unit, range, default }) }` | `Param(float, unit=, range=, default=)` | porta, `1: 2`, com `7` (`range`) e `8` (`default`) |
| sinal interno | `#[internal] Internal<R>` | `internals: { x: internal(R) }` | `Internal(R)` | porta, `1: 3`, com `9` (esquema) e `11` (domínio) |
| caixa de correio | `#[mailbox(to)] Mailbox<R, N>` | `mailboxes: { x: mailbox(R, { capacity, to }) }` | `Mailbox(R, capacity=, to=)` | porta, `1: 9`, com `12` |
| evento de entrada | `#[input(event)] EventInput<R>` | `inputs: { x: input.event(R) }` | `EventInput(R)` | porta, `1: 6` |
| evento de saída | `#[output(event)] EventOutput<R>` | `outputs: { x: output.event(R) }` | `EventOutput(R)` | porta, `1: 7` |
| ação (usar) | `#[action] Action<A, R>` | `actions: { x: action({ args, result }) }` | `Action(A, R)` | porta, `1: 8`, com `13` |
| alarme | `#[alarm(severity, message)] Alarm` | `alarms: { x: alarm({ severity, message }) }` | `Alarm(severity=, message=)` | porta, `1: 10`, com `14`; id `app.<app>.<nome>` |
| publicado | `#[publish] Published<T>` / `Counter` | `published: { x: published(output.f64(...)), n: counter() }` | `Published(float, ...)` / `Counter()` | porta, `1: 4` / `1: 5` |
| segredo | `#[joker::app(secrets = [...], optional_secrets = [...])]` | `secrets: [secret("mqtt_password")]` | `Secret("mqtt_password")` | chave 13 |
| modelo (1.1) | `#[model] Model` | `model({...})` | `Model(...)` | porta, `1: 11`; chave 10 com `"ml"` |
| equipamento | `#[device(requires)] Pump` | `devices: { p: device(Pump, { requires }) }` | `p = device(Pump, requires=)` | chave 8 |
| vários equipamentos | `Many<Pump, 1, 8>` | `many(Pump, { min: 1, max: 8 })` | `devices(Pump, 1, 8)` | chave 8, com `4` (`many`) |
| implementar classe | `#[joker::implements(Pump, capabilities)]` | `implements: implementsClass(Pump, { capabilities })` | `@implements(Pump, capabilities=)` | chave 9, com o hash do contrato |
| classe local | `#[derive(joker::Class)]` | `localClass({...})` | `@local_class` | chave 8, `class-ref = "local:<Nome>"` |
| tarefa de ciclo | `#[joker::cycle(period, wcet, init)]` | `cycle({ period, wcet, state }, fn)` | `@cycle(period, wcet=, state=, init=)` | chave 7, `1: 0` |
| tarefa async | `#[joker::task(every/on/on_event/at/startup)]` | `task.every/on/onEvent/at/startup` | `@task.every/on/on_event/at/startup` | chave 7, `1: 1`, com `6` (`trigger`) |
| estado persistente | `#[joker::state(persist)]` | `cycle({ persist: true })` | `@state(persist=True)` | chave 14 |
| ações de uma instância virtual | `#[joker::on_action(<ação>)]` | `onAction: { reset: … }` | `@on_action("reset")` | chave 7, `1: 2`, com `7` |

### 5.5 Validação das declarações

Feita pelo SDK durante a execução da declaração no `jk build` (§18.4). Todos os erros aparecem antes de haver `.wasm`.

| Regra | Código |
|---|---|
| nome da app, porta, tarefa, parâmetro, campo de registo, variante: gramática `[a-z][a-z0-9_]*`, ≤ 32 caracteres (02 §9, 16 P5) | `J-E0857` |
| nome reservado para porta ou tarefa: `now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta` | `J-E0857` |
| dois elementos com o mesmo nome na app (portas, tarefas, equipamentos, alarmes partilham um espaço) | `J-E0857` |
| unidade fora do catálogo (16 §5) | `J0109` |
| grandeza (`quantity`) sem unidade da dimensão certa | `J0108` |
| `range` com `min > max`, `default` fora da `range` | `J-E0851` |
| `Internal` com um campo não escalar, ou tamanho CBOR máximo > 64 B (ref/00 §10) | `J-E0804` |
| registo de evento ou argumento com `string` sem limite > 1 KiB ou lista > 64 (ref/00 §10) | `J-E0805` |
| `@cycle` sobre `async def`, `@task.*` sobre `def`, assinatura errada, `task.on` sobre uma porta que não é entrada | `J-E0813` |
| `version` do código diferente da do `pyproject.toml`/`package.json` | `J-E0852` |
| `@app` sem herdar de `App`; zero ou mais do que uma app no ponto de entrada | `J-E0850` |
| `@on_action` sem `@implements`, ou para uma ação que a classe não tem | `J-E0813` |
| chamada ao host durante a declaração (ler o relógio, `kv`, uma porta) | `J-E0810` |

---

## 6. Portas: leitura e qualidade

### 6.1 Acessores

Os acessores seguem a tabela normativa do documento 16, §3.1, sem exceções:

| Python | TypeScript | `good` | `uncertain` | `bad` | Domínios |
|---|---|---|---|---|---|
| `get()` | `get()` | valor | `QualityError` | `QualityError` | ciclo, async |
| `get_uncertain()` | `getUncertain()` | valor | valor | `QualityError` | ciclo, async |
| `get_or(d)` | `getOr(d)` | valor | `d` | `d` | ciclo, async |
| `sample()` | `sample()` | amostra | amostra | amostra | ciclo, async |
| — | `tryGet()` | `{ ok: true, value }` | `{ ok: false, error }` | `{ ok: false, error }` | ciclo, async |

- **No ciclo**, todos leem do latch do tick: duas leituras no mesmo tick dão a mesma amostra (02 §6.1). O SDK descodifica a imagem empacotada que o `on-cycle` recebe e não chama o host.
- **Em async**, leem o slot atual (`async-io.read`), e duas leituras seguidas podem diferir (02 §6.4). Para ler vários sinais de forma coerente, `self.snapshot(a, b)` / `io.snapshot([io.a, io.b])`.
- `get_or` regista no traço que a leitura foi substituída (16 §3.1). No ciclo, o SDK acrescenta ao buffer de propostas um registo `NOTE` (tipo 6, ref/04 §3.3); em async, chama `async-io.note-substituted`.
- `tryGet()` só existe em TS: é o `Result` do Rust para quem prefere não usar exceções. Em Python não há equivalente, porque a exceção é a forma idiomática e um segundo caminho só confundiria.
- `QualityError` traz a porta e a amostra completa; `str(e)` é `"J-E0800: port 'soc' is bad(comm_failure)"`.

### 6.2 Handles de entrada, parâmetro e interno

```python
class InputHandle(Generic[T]):
    def get(self) -> T: ...
    def get_uncertain(self) -> T: ...
    def get_or(self, default: T) -> T: ...
    def sample(self) -> Sample[T]: ...
    # só async
    async def history(self, start: Instant | str | None = None, end: Instant | str | None = None, *,
                      last: DurationLike | None = None, step: DurationLike | None = None,
                      agg: Agg = "mean") -> History[T]: ...

Agg = Literal["raw", "mean", "min", "max", "first", "last", "count"]   # history-query.agg (ref/04 §2.4)

class ParamHandle(Generic[T]):
    def get(self) -> T: ...                           # parâmetros são sempre válidos: a gama é do jk check (J0120)

class InternalHandle(Generic[R]):
    def get(self) -> R: ...                           # QualityError enquanto for uncertain(initial)
    def get_or(self, default: R) -> R: ...
    def get_or_default(self) -> R: ...                # o registo com o valor zero de cada campo (§6.2)
    def sample(self) -> Sample[R]: ...
    def set(self, value: R) -> None: ...              # ciclo e async; sem resultado

class History(Sequence[Sample[T]]):
    def values(self, *, quality: Literal["good", "uncertain"] = "good") -> list[T]: ...   # só as amostras com essa qualidade ou melhor
    def mean(self) -> float: ...                      # média das amostras good; QualityError se não houver nenhuma
    def coverage(self) -> float: ...                  # fração de amostras good, 0..1
```

```ts
export interface InputCycle<T> { get(): T; getUncertain(): T; getOr(d: T): T; sample(): Sample<T>; tryGet(): Result<T, QualityError> }
export interface InputTask<T> extends InputCycle<T> {
  history(q: { start?: Instant | string; end?: Instant | string; last?: DurationLike; step?: DurationLike; agg?: Agg }): Promise<History<T>>;
}
export type Agg = "raw" | "mean" | "min" | "max" | "first" | "last" | "count";
export interface ParamHandle<T> { get(): T }
export interface InternalHandle<R> { get(): R; getOr(d: R): R; getOrDefault(): R; sample(): Sample<R>; set(v: R): void }
export interface History<T> extends ReadonlyArray<Sample<T>> {
  values(o?: { quality?: "good" | "uncertain" }): T[]; mean(): number; coverage(): number;
}
export type Result<T, E> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: E };
```

`get_or_default()` devolve o registo com o **valor zero** de cada campo (`False`, `0`, `0.0`, `""`, `b""`, a variante de valor 0 de um enum, `None` nos opcionais), como o `Default` derivado do Rust, e não os valores por omissão do construtor Python: assim as três linguagens dão o mesmo registo.

`history()` devolve as amostras do historian no intervalo (`async-io.history`, ref/04 §2.4). Com `step`, uma amostra por passo, agregada por `agg` (omissão `mean`), com a **qualidade dominante** do passo; sem `step`, `agg` tem de ser `raw` e vêm os pontos crus. `last="24h"` é `[now − 24h, now]`. `start`/`end` como string aceitam RFC 3339 (ref/00 §3). Mais de 10 000 pontos é `J-E0402`.

### 6.3 Portas múltiplas

`InputMany`/`OutputMany` dão um `ManyPorts[H]` (Python) ou `ManyPorts<H>` (TS): uma sequência imutável de handles, ordenada pelo caminho do sinal ligado, com o tamanho fixado no `init` (04 §3.1). Cada elemento tem `.path` (o caminho ligado, em texto) e `.index`.

---

## 7. Escritas e resultados

### 7.1 Saídas

| Python | TypeScript | Domínio | Semântica |
|---|---|---|---|
| `set(v)` | `set(v)` | ciclo | proposta no buffer do tick; a última do tick vence (16 A4); sem resultado |
| `set(v)` | `set(v)` | async | proposta sem esperar pelo resultado (`async-io.set`): vai para a inbox do grupo dono; `last_outcome()` dá `deferred` até ao commit |
| `relinquish()` | `relinquish()` | ciclo, async | liberta o nível já (01 §6) |
| `pending()` | `pending()` | ciclo | a proposta desta app neste tick, ou `None`/`null`; `None` também para saídas escritas só por métodos (17 §4.1) |
| `last_outcome()` | `lastOutcome()` | ciclo, async | o `WriteOutcome` da proposta deste escritor no commit mais recente do grupo dono, ou `None`/`null` (02 §7.1) |
| `get()`, `sample()` | `get()`, `sample()` | ciclo, async | o valor **aplicado** (17 §11), com qualidade |
| `await write(v)` | `await write(v)` | async | proposta para o grupo dono; resolve com o resultado **final**, depois do commit (01 §5) |
| `set_at(v, at)` | `setAt(v, at)` | ciclo | escrita agendada (M6), **v1.1** |
| `await write_at(v, at)` | `await writeAt(v, at)` | async | idem, **v1.1** |
| `await propose(v, reason=, ttl=)` | `await propose(v, { reason, ttl })` | async | intenção (M18), **v1.1**; devolve o `IntentId` (ULID) |

```python
class OutputHandle(Generic[T]):
    def set(self, value: T) -> None: ...
    def relinquish(self) -> None: ...
    def pending(self) -> T | None: ...
    def last_outcome(self) -> WriteOutcome | None: ...
    def get(self) -> T: ...
    def sample(self) -> Sample[T]: ...
    async def write(self, value: T) -> WriteOutcome: ...
    def set_at(self, value: T, at: Instant | int) -> None: ...                     # v1.1
    async def write_at(self, value: T, at: Instant | int) -> WriteOutcome: ...     # v1.1
    async def propose(self, value: T, *, reason: str, ttl: DurationLike) -> str: ...   # v1.1
```

```ts
export interface OutputCycle<T> {
  set(v: T): void; relinquish(): void; pending(): T | null; lastOutcome(): WriteOutcome | null;
  get(): T; sample(): Sample<T>;
  setAt(v: T, at: Instant | bigint): void;                                          // v1.1
}
export interface OutputTask<T> {
  set(v: T): void; write(v: T): Promise<WriteOutcome>; relinquish(): void; lastOutcome(): WriteOutcome | null;
  get(): T; sample(): Sample<T>;
  writeAt(v: T, at: Instant | bigint): Promise<WriteOutcome>;                       // v1.1
  propose(v: T, o: { reason: string; ttl: DurationLike }): Promise<string>;        // v1.1
}
```

- **Um resultado mau não é uma exceção.** `rejected`, `tripped` e `overridden` são resultados normais, como no Rust. `outcome.ok` diz se algum valor desta proposta foi aplicado.
- **Valores inválidos são exceções.** Um valor fora do tipo (`u64` negativo, `i64` acima de 2⁶³−1, `string` com mais de 64 B, `number` não inteiro numa porta `i64`) levanta `LimitError` (`J-E0806`, `J-E0805`) **antes** de chegar ao buffer. Um `NaN` ou infinito numa saída `f64` é um valor válido para o SDK e é o envelope que o trata (passo 1, `Rejected(WrongType)`).
- **Em async, esquecer o `await`** de `write()` não perde a escrita: a proposta é enviada no momento da chamada, e o `await` só espera pelo resultado. O Python avisa pelo `RuntimeWarning` normal de corrotina não aguardada; o TS com a regra `no-floating-promises` que o modelo do `jk app new` liga.
- As APIs da v1.1 existem nos tipos desde a 1.0 e comportam-se **como no SDK Rust** (ref/04 §2.9): o SDK marca a funcionalidade instável no `joker:meta` (chave 10) quando a função é referenciada (§18.4); em `mode = "prod"`, o `jokerd` recusa o componente (`J-E0003`, `error-in-prod`), e o `jk check` avisa antes do deploy; em `mode = "dev"`, a runtime liga-a a stubs que devolvem `J-E0601`, e o SDK levanta `UnavailableError` com esse código. O `J-E0809` fica para outro caso: uma API **estável** de um mundo posterior àquele para que a app foi gerada (E10).

### 7.2 Publicados, contadores e alarmes

```python
class PublishedHandle(Generic[T]):
    def set(self, value: T) -> None: ...             # ciclo e async

class CounterHandle:
    def add(self, n: int = 1) -> None: ...           # counter-add; n ≥ 0; n < 0 → LimitError (J-E0806)

class AlarmHandle:
    def raise_(self) -> None: ...                    # PEP 8: sufixo _ para palavras reservadas
    def clear(self) -> None: ...
    def set(self, active: bool) -> None: ...
    def state(self) -> AlarmState: ...               # normal | active_unacked | active_acked | rtn_unacked | unknown (ref/00 §6)
```

```ts
export interface PublishedHandle<T> { set(v: T): void }
export interface CounterHandle { add(n?: bigint | number): void }
export interface AlarmHandle { raise(): void; clear(): void; set(active: boolean): void; state(): AlarmState }
export type AlarmState = "normal" | "active_unacked" | "active_acked" | "rtn_unacked" | "unknown";
```

Um alarme de app é uma **condição** que a app escreve; o ciclo de vida (atrasos, reconhecimento, shelving, supressão) é o do núcleo (02 §11). `raise_()`/`raise()` e `clear()` são açúcar sobre `set(True)`/`set(False)`. No mundo WIT, o alarme é uma porta `alarm` (`port-kind` 10), escrita com `publish` e um `bool`; `state()` lê o sinal `alarm.app.<app>.<nome>.state` (do latch no ciclo, do slot em async). O id do alarme na API e na `jk` é `app.<app>.<nome>` (ref/04 §4.2). O Rust não tem `get()` num contador, e por isso os SDKs de tier 2 também não: quem precisa do valor lê o sinal publicado por uma porta de entrada.

---

## 8. Parâmetros

- `ParamHandle.get()` devolve sempre um valor: os parâmetros vêm do manifesto (`[apps.<a>.params]`), com a unidade convertida para a da porta, a gama verificada no `jk check` (`J0120`) e o `default` da declaração quando o manifesto não o dá. Não há qualidade.
- Um parâmetro sem `default` e sem valor no manifesto é `J0142` no `jk check`.
- **Mudança a quente** (`on-params`): os valores novos valem a partir do tick seguinte. Se a app declarar `@on_params` / `onParams`, o SDK chama-o antes de aceitar os valores; uma exceção rejeita-os (`on-params` devolve `Err` com a mensagem, e a geração é recusada para esta app com `J-E0011`, ref/04 §2.6.1).
- `Param(float, unit="%", range=(0, 50))` guarda o valor em `%`. Para uma fração, divide-se por 100 no código, como no documento 04, §3.2.

---

## 9. Tarefas

### 9.1 Tarefas de ciclo

```python
@state(persist=True)                       # opcional; sem persist, o estado não sobrevive ao hot-swap
class Ctl:
    integral: float = 0.0

@cycle("200ms", wcet="20ms", state=Ctl, init=None, optional=False)
def control(self, st: Ctl) -> None: ...
```

```ts
cycles: {
  control: cycle({ period: "200ms", wcet: "20ms", state: (io) => ({ integral: 0 }), persist: true }, (io, st) => { … }),
}
```

| Opção | Tipo | Omissão | Semântica |
|---|---|---|---|
| `period` | duração | obrigatório | período **pedido**; o grupo é do manifesto (`[apps.<a>.cycles]`), e uma divergência > `period_tolerance` é `J0130` (04 §3.2). Mínimo em tier 2: §19 (`J-E0854`) |
| `wcet` | duração | 20% do `period` | WCET declarado para a admissão (M23), até o `jk bench` o medir; > 50% do período é `J-E0854` |
| `state` (Python: classe; TS: fábrica) | — | nenhum | o estado local da tarefa; vive na memória da instância de ciclo |
| `init` (Python) | `(self) -> S` | `S()` | corre uma vez, dentro do primeiro `on-cycle` da tarefa (com latch e parâmetros disponíveis); em TS, a fábrica `state(io)` faz o mesmo |
| `persist` | `bool` | `false` | exporta o estado no `export-state` e restaura-o no `migrate` (§9.4) |
| `optional` | `bool` | `false` | tarefa que o `overrun = "degrade"` pode desligar (01 §4) |

Regras:

- **Não há `async` nem I/O no ciclo.** Em Python, uma tarefa de ciclo é um `def`; em TS, a função devolve `void` e um `Promise` devolvido é `J-E0813` no build (e `J-E0901` em runtime, se passar por `any`).
- **Estado:** o `st` é o mesmo objeto em todos os ticks. Em Python, `self` é só leitura (`J-E0803`). Em TS, `io` é congelado (`Object.freeze`), e uma atribuição em modo estrito lança `TypeError` com a mensagem de `J-E0803`.
- **Relógio e aleatoriedade determinísticos.** Na instância de ciclo, `Date.now()`, `performance.now()`, `time.time()`, `time.monotonic()` e `time.time_ns()` devolvem o instante do latch (o mesmo de `now()`), porque o host serve `wasi:clocks` com ele (ref/04 §2.7). `Math.random()` e o módulo `random` são semeados no `init` a partir de `wasi:random`, que o host serve com o gerador determinista semeado pela geração (ref/04 §2.7: não é gravado no traço, reproduz-se a partir do `generation-hash`). O replay é exato.
- **Memória:** o SDK não repõe a memória no fim do tick (há GC). O heap da instância é pré-alocado com o `memory.max` e fixado (01 §9); as pausas de GC acontecem dentro do tick e contam para o WCET (§19).

### 9.2 Tarefas async

| Gatilho | Python | TypeScript | Handler | Semântica |
|---|---|---|---|---|
| periódico | `@task.every("15m", jitter="30s")` | `task.every("15m", { jitter: "30s" }, fn)` | `(self)` / `(io)` | se a execução anterior ainda corre, o disparo fica pendente (um só) e os seguintes contam em `joker.app.<app>.task.<t>.coalesced` (ref/04 §4.3.2) |
| cron | `@task.at("0 */15 * * * *")` | `task.at("0 */15 * * * *", fn)` | idem | cron de 6 campos, UTC; exige relógio `synced` ou `holdover`, e com `unsynced` o disparo espera (ref/04 §4.3.2) |
| mudança | `@task.on("soc")` | `task.on("soc", fn)` | `(self, s: Sample[T])` / `(io, s)` | uma chamada por amostra nova; com o handler ocupado, fica só a mais recente (*latest-wins*) e as outras são contadas como perdidas |
| evento | `@task.on_event("detections")` | `task.onEvent("detections", fn)` | `(self, e: R)` / `(io, e)` | cada evento é entregue, por ordem; a fila é a do consumidor (02 §1) |
| arranque | `@task.startup()` | `task.startup(fn)` | `(self)` / `(io)` | uma vez, depois do `init` |

- As tarefas async de uma app correm **concorrentemente** no mesmo *event loop* (asyncio em Python, o *event loop* do motor em TS), uma de cada vez entre dois `await`. Não há threads (`threading.Thread.start()` e `Worker` falham).
- **Como os gatilhos chegam ao guest** (ref/04 §2.6.1, §4.3.2): o host chama `run-task(task, why)` uma vez por disparo, em todos os gatilhos: `timer` para `every` e `at`, `change` para `on`, `event` para `on_event`, `startup`. O SDK despacha para o handler com a amostra ou o evento já descodificados. Um disparo que chega com o handler a correr fica pendente (um só; as mudanças juntam-se na mais recente) e os perdidos contam em `joker.app.<app>.task.<t>.coalesced`. As funções `subscribe()` e `events()` continuam disponíveis para quem quer consumir um fluxo dentro de uma tarefa.
- Na instância async, os relógios de `Date.now()` e `time.time()` são os reais, e `now()` também. O replay de async é o das propostas na fronteira, ou exato com `trace.async = true` (09 ataque 10).

### 9.3 Erros numa tarefa

| O que acontece | Ciclo | Async |
|---|---|---|
| a tarefa termina normalmente | as propostas do tick seguem para o árbitro | — |
| um `JokerError` sai da tarefa (incluindo `QualityError`) | a tarefa termina **neste tick**; as propostas já feitas **ficam** (`cycle-result.error` com o código, ref/04 §2.6.1); conta em `joker.app.<a>.task.<t>.errors` e fica no log com o código | a execução termina (`run-task` devolve `Err`); fica no log; o gatilho continua |
| qualquer outra exceção | equivalente a um `panic`: as propostas desta app **neste tick são descartadas** (01 §9); conta para os K traps do crashloop; o log leva o *traceback* | a execução termina; conta em `joker.app.<a>.task.<t>.failures`; o log leva o *traceback*; a instância continua |
| trap real do Wasm (`memory.grow` falha, pilha, deadline por epoch) | trap (01 §9) | trap; a instância async é recriada |

A distinção é a do Rust: um `JokerError` é um `Err` esperado (dados maus), e o resto é um bug. Uma exceção não apanhada **não** faz trap à instância: o SDK apanha-a e devolve `cycle-result` com `proposals` vazio e `error.code = J-E0816` (ou `Err(J-E0816)` em `run-task`); o host conta-o como um trap para o crashloop, sem recriar a instância, porque recriar uma instância Python custa centenas de ms (ref/04 §2.6.1). Para os `JokerError` no ciclo, o SDK devolve as propostas do tick e o erro no mesmo `cycle-result`: não há registo de erro no buffer.

### 9.4 Estado persistente e migração

- Com `persist`, o SDK codifica o estado de cada tarefa de ciclo em CBOR determinístico no `export-state`, num mapa `{ "<tarefa>": estado }`, e os `Many` por nome de instância (14 A10). Tamanho máximo: 64 KiB (`J-E0815` acima).
- No `migrate(previous, from_version)`, o SDK descodifica o mapa e, se a app declarar `@migrate`/`migrate`, chama-o com o estado anterior (já descodificado em `dict`/objeto) e a versão; o valor devolvido é o estado novo. Sem hook, o SDK restaura os campos com o mesmo nome e tipo e usa o valor por omissão nos outros. Um campo com tipo incompatível e sem hook é `J-E0815` e a versão nova não entra (01 §9, "se o `init` falhar, a antiga continua").
- Os campos de um estado persistente têm de ser codificáveis em CBOR: escalares, `str`/`string`, `bytes`, listas, mapas com chaves de texto e registos. Um objeto qualquer é `J-E0815` no primeiro `export-state` e, se o SDK o conseguir ver pelas anotações, já no build.

---

## 10. Partilha entre ciclo e async

As duas instâncias não partilham memória (01 §5, 09 ataque 6). Os três canais são os do documento 04, §3.4:

| Canal | Python | TypeScript | Semântica |
|---|---|---|---|
| sinal interno | `Internal(R)` → `get()`, `set(r)` | `internal(R)` → `get()`, `set(r)` | último valor, ≤ 64 B, seqlock; o ciclo lê do latch |
| caixa de correio | `Mailbox(R, capacity=N, to="async")` | `mailbox(R, { capacity: N, to: "async" })` | fila SPSC limitada (`mailbox-send`, `mailbox-recv`, `mailbox-next`, ref/04 §2.3–§2.4); `try_send`/`trySend` nunca bloqueia e devolve `False` se estiver cheia; as perdas são contadas pelo emissor |
| eventos | `EventOutput(R).emit(r)` | `output.event(R)` → `emit(r)` | fila por consumidor |

```python
class MailboxHandle(Generic[R]):
    def try_send(self, item: R) -> bool: ...          # no domínio que envia (mailbox-send)
    def lost(self) -> int: ...                        # no domínio que envia: envios recusados por fila cheia
    def try_recv(self) -> R | None: ...               # no domínio que recebe (mailbox-recv)
    def drain(self) -> list[R]: ...                   # idem, mailbox-recv até vazia
    async def recv(self) -> R: ...                    # só quando o recetor é async (mailbox-next)

class EventOutputHandle(Generic[R]):
    def emit(self, event: R) -> None: ...             # ciclo e async

class EventInputHandle(Generic[R]):
    def events(self) -> AsyncIterator[R]: ...         # só async
```

```ts
export interface MailboxSend<R> { trySend(item: R): boolean; lost(): bigint }
export interface MailboxRecvCycle<R> { tryRecv(): R | null; drain(): R[] }
export interface MailboxRecvTask<R> extends MailboxRecvCycle<R> { recv(): Promise<R> }
export interface EventOutputHandle<R> { emit(e: R): void }
export interface EventInputHandle<R> { events(): AsyncIterable<R> }
```

Em TS o tipo de `io.<caixa>` depende do domínio e de `to`: do lado que envia só há `trySend`, do lado que recebe só há `tryRecv`/`drain`/`recv`. Em Python o handle tem os dois lados e o do domínio errado levanta `DomainError` (`J-E0802`).

**Variáveis globais do módulo.** O mesmo módulo é carregado nas duas instâncias, e uma variável global escrita numa tarefa async nunca é vista pelo ciclo (nem ao contrário). O SDK não o consegue impedir em runtime. O `jk build` avisa (`J-E0858`) quando uma função decorada como tarefa escreve numa global do módulo (`global x` em Python; atribuição a um `let` do módulo em TS).

---

## 11. Ações

**Usar uma ação** (uma porta `Action`, ou a ação de uma instância de classe, §14): só em async, porque espera por resposta (02 §8).

```python
class ActionHandle(Generic[A, R]):
    async def invoke(self, args: A | None = None, *, key: str | None = None,
                     timeout: DurationLike | None = None) -> R: ...                      # async-io.invoke
    def invoke_long(self, args: A | None = None, *, key: str | None = None) -> LongAction[R]: ...  # start-action

class LongAction(Generic[R]):                         # recurso action-call (ref/04 §2.4)
    def progress(self) -> AsyncIterator[tuple[float | None, str]]: ...
    def cancel(self) -> None: ...
    async def finish(self) -> R: ...                  # ActionError(kind="cancelled") se foi cancelada
```

```ts
export interface ActionHandle<A, R> {
  invoke(...a: A extends void ? [o?: InvokeOpts] : [args: A, o?: InvokeOpts]): Promise<R>;
  invokeLong(...a: A extends void ? [o?: { key?: string }] : [args: A, o?: { key?: string }]): LongAction<R>;
}
export interface InvokeOpts { key?: string; timeout?: DurationLike }
export interface LongAction<R> {
  progress(): AsyncIterable<{ fraction: number | null; message: string }>;
  cancel(): void;
  finish(): Promise<R>;
}
```

- `key` é a chave de idempotência (ref/00 §5: ASCII imprimível, 1–64 B; `J-E0805` fora disso). Sem `key`, o SDK gera uma (`"<app>:<porta>:<ULID>"`) por chamada. Para uma nova tentativa depois de um `timeout`, a app reutiliza a mesma chave, e o núcleo devolve o resultado anterior dentro do `dedup_window` (16 E9).
- `timeout` só pode encurtar o do manifesto; nunca o alonga.
- Erros: `ActionError` com o `code` que o host der (`J-E0301`…`J-E0313`) e `kind` derivado dele pela tabela da ref/04 §7 (`denied`, `forbidden`, `timeout`, `device`, `invalid`, `busy`, `not_connected`, `cancelled`, `conflict`, `unsupported`, `failed`, `other`).
- No ciclo, as ações não existem nos tipos TS, e em Python levantam `DomainError` (`J-E0901`) e são apanhadas pelo `J-E0859` no build. Uma tarefa de ciclo que precise de uma ação publica um evento ou usa uma `Mailbox`, e uma tarefa async executa-a (02 §8).
- Ações longas (`@long`, 17 §4.2): `invoke_long()`/`invokeLong()` sobre `async-io.start-action` e o recurso `action-call` (ref/04 §2.4). `invoke_long` numa ação que não é longa é `ActionError` com `J-E0313`.

**Implementar uma ação** (instância virtual, 17 §5.3): §14.5.

---

## 12. Relógio, `kv`, log, segredos e rede

### 12.1 `kv` (só async)

```python
class Kv:
    def get(self, key: str) -> bytes | None: ...
    async def set(self, key: str, value: bytes) -> None: ...
    async def delete(self, key: str) -> None: ...
    def keys(self, prefix: str = "") -> list[str]: ...
    def batch(self) -> KvBatch: ...                   # .set(k, v).delete(k); await .commit() (kv.commit, ≤ 64 operações)
    def get_record(self, key: str, type: type[R]) -> R | None: ...
    async def set_record(self, key: str, value: R) -> None: ...
```

```ts
export interface Kv {
  get(key: string): Uint8Array | null;
  set(key: string, value: Uint8Array): Promise<void>;
  delete(key: string): Promise<void>;
  keys(prefix?: string): string[];
  batch(): { set(k: string, v: Uint8Array): unknown; delete(k: string): unknown; commit(): Promise<void> };
  getRecord<R>(key: string, r: RecordSchema<R>): R | null;
  setRecord<R>(key: string, r: RecordSchema<R>, value: R): Promise<void>;
}
```

| Regra | Valor | Código |
|---|---|---|
| chave | 1–128 B, `[A-Za-z0-9._:/-]` | `J-E0811` |
| valor | ≤ 64 KiB | `J-E0811` |
| total por app | ≤ 1 MiB (01 §9.1) | o host recusa: `KvError` com `J-E0404` (ref/04 §2.5.1) |
| no ciclo | não existe (01 §9.1) | `J-E0901` |

`get` é síncrono porque o WIT o é (04 §2); `set` e `delete` só resolvem depois do fsync da política do `kv`. Os registos usam o mesmo CBOR dos eventos (§15.2).

### 12.2 Log

```python
from joker_sdk import log
log.info("plano novo", target_soc=0.62, source="history")     # também trace, debug, warn, error
```

```ts
import { log } from "@joker/sdk";
log.info("plano novo", { target_soc: 0.62, source: "history" });
```

- Os campos vão como `list<tuple<string, string>>` (`log.log`, ref/04 §2.5.3): o SDK converte cada valor com `str()`/`String()`. A mensagem é truncada a 1 KiB e os campos a 16, com a marca `…` no fim (sem erro).
- `log.enabled("debug")` / `log.enabled("debug")` é o `log.enabled` do WIT: o SDK usa-o para não formatar mensagens que o host deitaria fora.
- Funciona nos dois domínios; no ciclo vai para o anel lock-free (09 ataque 5).
- Em Python, o SDK instala um handler no `logging` (os `logging.getLogger(__name__)` das bibliotecas acabam no mesmo sítio) e `print()` vai para o log com `stream=stdout`. Em TS, `console.log/info/warn/error` fazem o mesmo.
- Os nomes dos campos são dados do utilizador: ficam em `snake_case`, mesmo em TS.

### 12.3 Segredos

Um segredo é declarado pelo nome local (`Secret("mqtt_password")`, `secret("mqtt_password")`; chave 13 do `joker:meta`), ligado no manifesto em `[apps.<app>.secrets] mqtt_password = "secret:<nome>"` (D6, ref/01) e lido só em async: `await self.secrets.get("mqtt_password")` / `await io.secrets.get("mqtt_password")`, sobre `secrets.reveal` (ref/04 §2.5.2). Devolve um `SecretStr` (Python) ou `SecretString` (TS) cujo `repr`/`toString`/`toJSON` é `"***"`; o valor lê-se com `.reveal()`. Ler um nome que a app não declarou é `J-E0817`, sem chamar o host; declarado mas não ligado é `J-E0408`, e ligado mas ausente no nó é `J-E0409` (ref/04). O manifesto nunca tem o valor (01 §17, ref/00 §9).

### 12.4 Rede (só async)

- **TS:** `fetch()` global, implementado sobre `wasi:http/outgoing-handler`. Só alcança os destinos de `[apps.<a>.net] allow` (03 §8); o resto falha com um `TypeError` do `fetch` cuja causa é um `HostError` com o código do host. No ciclo, `fetch` lança `DomainError` (`J-E0901`).
- **Python:** `joker_sdk.http` (`async def request(method, url, *, headers=None, body=None, timeout=None) -> Response`, mais `get` e `post`), sobre o mesmo `wasi:http`. `socket`, `urllib.request`, `requests` e `httpx` não funcionam, porque não há sockets no componente.
- Não há sistema de ficheiros. Os ficheiros de dados declarados em `include` (§18) leem-se com `importlib.resources` (Python) ou `import … with { type: "json" }` (TS, embutido pelo bundler).

---

## 13. A vista de I/O

**Python.** `self`, dentro de uma tarefa, é a vista do domínio em que a instância corre. Os atributos de classe (descritores) devolvem handles; os métodos de `App` estão no §5.2. Chamar uma função do outro domínio levanta `DomainError`: `J-E0901` (API async no ciclo) ou `J-E0802` (API de ciclo em async, como `dt()` ou `pending()`).

**TypeScript.** `io` é um objeto congelado com as portas da definição e os serviços do domínio:

```ts
export type CycleIo<P> = { readonly [K in keyof P]: CycleHandleOf<P[K]> } & {
  now(): Instant;
  dt(): Duration;
};
export type TaskIo<P> = { readonly [K in keyof P]: TaskHandleOf<P[K]> } & {
  now(): Instant;
  snapshot<T extends readonly Readable<any>[]>(ports: T): { [I in keyof T]: Sample<ReadValue<T[I]>> };
  subscribe(ports: readonly Readable<any>[]): AsyncIterable<Change>;
  sleep(d: DurationLike): Promise<void>;
  sleepUntil(t: Instant): Promise<void>;
  readonly kv: Kv;
  readonly secrets: Secrets;
};
export interface Change { readonly port: string; readonly sample: Sample<unknown> }

type CycleHandleOf<D> =
  D extends InputDecl<infer T> ? InputCycle<T> :
  D extends InputManyDecl<infer T> ? ManyPorts<InputCycle<T>> :
  D extends OutputDecl<infer T> ? OutputCycle<T> :
  D extends OutputManyDecl<infer T> ? ManyPorts<OutputCycle<T>> :
  D extends ParamDecl<infer T> ? ParamHandle<T> :
  D extends InternalDecl<infer R> ? InternalHandle<R> :
  D extends MailboxDecl<infer R, "async"> ? MailboxSend<R> :
  D extends MailboxDecl<infer R, "cycle"> ? MailboxRecvCycle<R> :
  D extends EventOutputDecl<infer R> ? EventOutputHandle<R> :
  D extends AlarmDecl ? AlarmHandle :
  D extends PublishedDecl<infer T> ? PublishedHandle<T> :
  D extends CounterDecl ? CounterHandle :
  D extends DeviceDecl<ClassSchema<infer Cy, any, any>, infer Req> ? CycleView<Cy, Req> :
  D extends ManyDecl<ClassSchema<infer Cy, any, any>, infer Req> ? Many<CycleView<Cy, Req>> :
  never;                                   // EventInput e Action não existem no ciclo
type TaskHandleOf<D> = /* o mesmo, com InputTask, OutputTask, MailboxRecvTask, EventInputHandle,
                          ActionHandle e TaskView das classes */ unknown;
```

Um `never` no tipo do ciclo é o que faz de `io.reset_line.invoke()` numa tarefa de ciclo um erro de compilação, como o A9 do documento 14 pede.

---

## 14. Classes de equipamento

### 14.1 O que o `jkc` gera

Para cada classe, o `jkc` (17 §6) gera um módulo por linguagem. Para `std/pump@1`:

```python
# joker_classes/std/pump.pyi (gerado)
class PumpState(Record):
    running: bool
    fault: bool
    fault_code: int | None
    speed: float | None               # capacidade variable_speed
    pressure_out: float | None        # capacidade pressure
    hours: float | None               # capacidade hour_counter

class PumpTripped(Record):
    code: int

class Pump(ClassHandle[PumpState]):
    CLASS: ClassVar[str] = "std/pump@1"
    CAPABILITIES: ClassVar[tuple[str, ...]] = ("variable_speed", "pressure", "hour_counter")
    # membros (camada 1 por baixo): in, param e query são leituras; out lê o valor aplicado
    run_cmd: OutMember[bool]
    running: Member[bool]
    fault: Member[bool]
    fault_code: Member[int]
    rated_flow: Member[float]
    failed_to_start: Member[bool]
    failed_to_stop: Member[bool]
    speed_set: OutMember[float]       # variable_speed
    speed: Member[float]              # variable_speed
    pressure_out: Member[float]       # pressure
    dry_running: Member[bool]         # pressure
    hours: Member[float]              # hour_counter
    tripped: EventMember[PumpTripped]
    # métodos: no ciclo, registo de método (sem resultado); em async, await → resultados das escritas expandidas
    def start(self) -> MethodTicket: ...
    def stop(self) -> MethodTicket: ...
    def run_at(self, pct: float) -> MethodTicket: ...        # pct em %
    # ações: só async
    async def reset(self, *, key: str | None = None, timeout: DurationLike | None = None) -> None: ...
```

```ts
// @joker/classes-std/pump.d.ts (gerado)
export interface PumpState {
  running: boolean; fault: boolean; fault_code: bigint | null;
  speed: number | null; pressure_out: number | null; hours: number | null;
}
export interface PumpTripped { code: bigint }
interface PumpCycleCore extends ClassHandleCycle<PumpState> {
  run_cmd: OutMember<boolean>; running: Member<boolean>; fault: Member<boolean>;
  fault_code: Member<bigint>; rated_flow: Member<number>;
  failed_to_start: Member<boolean>; failed_to_stop: Member<boolean>;
  start(): void; stop(): void;
}
interface PumpCycleVariableSpeed { speed_set: OutMember<number>; speed: Member<number>; run_at(pct: number): void }
interface PumpCyclePressure { pressure_out: Member<number>; dry_running: Member<boolean> }
interface PumpCycleHourCounter { hours: Member<number> }
export type PumpCycle<Req extends string> = PumpCycleCore
  & ("variable_speed" extends Req ? PumpCycleVariableSpeed : Partial<PumpCycleVariableSpeed>)
  & ("pressure" extends Req ? PumpCyclePressure : Partial<PumpCyclePressure>)
  & ("hour_counter" extends Req ? PumpCycleHourCounter : Partial<PumpCycleHourCounter>);
// PumpTask<Req>: os mesmos membros; start/stop/run_at devolvem Promise<WriteOutcome[]>;
// reset(o?: InvokeOpts): Promise<void>; tripped: EventMember<PumpTripped>
export declare const Pump: ClassSchema<PumpCycle<never>, PumpTask<never>, "variable_speed" | "pressure" | "hour_counter">;
```

Os tipos partilhados das classes:

```python
class Member(Generic[T]):                 # chamável: p.running() ≡ get()
    def __call__(self) -> T: ...
    def get(self) -> T: ...
    def get_uncertain(self) -> T: ...
    def get_or(self, default: T) -> T: ...
    def sample(self) -> Sample[T]: ...
    async def history(self, start=None, end=None, *, last=None, step) -> History[T]: ...   # só async

class OutMember(Member[T]): ...           # lê o valor aplicado (17 §11); escreve-se por métodos ou por raw()

class EventMember(Generic[R]):
    def events(self) -> AsyncIterator[R]: ...

class MethodTicket(Awaitable[list[WriteOutcome]]): ...   # no ciclo não se aguarda; em async, await

class HealthStatus(StrEnum):
    OK = "ok"; DEGRADED = "degraded"; FAILED = "failed"

@dataclass(frozen=True)
class Health:
    status: HealthStatus
    members: tuple[str, ...]              # os membros que não estão good

class ClassHandle(Generic[S]):
    name: str                             # nome da instância no manifesto
    index: int                            # posição num Many (0 fora de um Many)
    def health(self) -> Health: ...
    def read(self) -> S: ...              # QualityError se Failed; membros de capacidades não good → None
    def raw(self) -> Any: ...             # handles de camada 1 dos membros (14 §3): p.raw().run_cmd.set(True)
    def has(self, capability: str) -> bool: ...

class Many(Generic[H]):
    def __len__(self) -> int: ...
    def __iter__(self) -> Iterator[H]: ...
    def __getitem__(self, i: int | str) -> H: ...   # posição ou nome da instância
    def get(self, name: str) -> H: ...           # nome desconhecido → KeyError com J-E0807
    def at(self, i: int) -> H: ...               # fora da gama → IndexError com J-E0807
    def healthy(self) -> Iterator[H]: ...
    def read_all(self) -> list[Any]: ...         # QualityError se alguma estiver failed
```

```ts
export interface Member<T> { (): T; get(): T; getUncertain(): T; getOr(d: T): T; sample(): Sample<T>; tryGet(): Result<T, QualityError> }
export interface OutMember<T> extends Member<T> {}
export type Health = { readonly status: "ok" } | { readonly status: "degraded" | "failed"; readonly members: readonly string[] };
export interface ClassHandleCycle<S> {
  readonly name: string; readonly index: number;
  health(): Health; read(): S; raw(): Record<string, unknown>; has(capability: string): boolean;
}
export interface Many<H> extends Iterable<H> {
  readonly length: number; get(name: string): H; at(i: number): H; healthy(): Iterable<H>; readAll(): unknown[];
}
```

### 14.2 Semântica

| Operação | Ciclo | Async | Fonte |
|---|---|---|---|
| ler membro, consulta (`p.running()`, `p.failed_to_start()`) | latch | slot atual | 14 §6.4; as consultas são sinais derivados do núcleo (14 A7) |
| método (`p.start()`) | registo de método no buffer; `pending()` do `out` fica `None` | `await` devolve os `WriteOutcome` das escritas expandidas, depois do commit | 17 §4.1 |
| ação (`p.reset()`) | não existe | pedido/resposta, chave, timeout | 17 §4.2 |
| `health()` | regra de 17 §4.5 sobre os membros do latch | idem, sobre o slot atual | 17 §4.5 |
| `read()` | registo gerado; `QualityError` se `failed` | idem | 14 §6.3 |
| `Many` | ordem estável (manifesto, ou nome); tamanho fixo no `init` | idem | 14 §6.2 |
| `raw()` | handles de camada 1 dos membros | idem | 14 §3 |

- **`health()` é calculado pelo SDK**, com a regra do documento 17, §4.5: uma função pura das qualidades dos membros, sem estado nem tempo, que não é o caso que o A7 do documento 14 proíbe. Para garantir que Rust, TS e Python dão o mesmo resultado, os três SDKs passam os mesmos vetores de teste, em `conformance/sdk/health.json`.
- **Capacidades.** Em TS, os membros das capacidades listadas em `requires` são obrigatórios no tipo; os outros são opcionais (`Partial`), e um membro de uma capacidade que a instância não tem é `undefined`. Em Python, estão sempre no tipo; aceder a um membro de uma capacidade que a instância não tem levanta `CapabilityError` (`J-E0808`), e `p.has("pressure")` permite testar. Com `requires`, o `jk check` garante-os (`J0602`).
- **Unidades dos argumentos de métodos:** a unidade da classe (`run_at(pct: f64 %)` recebe `60.0` para 60 %).
- **Colisões de nomes.** Os nomes da API do handle (`name`, `index`, `health`, `read`, `raw`, `has`) são reservados para membros de classes (ref/00 §2, documento 17 §3.2): o `jkc` recusa-os. Um membro com o nome de uma palavra reservada da linguagem (Python: `from`, `in`, `class`, …; TS: nenhuma, porque as propriedades aceitam palavras reservadas) ganha um `_` no fim no código gerado (`p.from_()`), e o `jkc` avisa.

### 14.3 Usar classes

```python
from joker_classes.std import Led, Button

@app(name="hello_world", version="0.5.0")
class HelloWorld(App):
    led = device(Led)
    botao = device(Button)

    @cycle("500ms")
    def control(self) -> None:
        if self.botao.clicked():
            self.led.toggle()
```

O exemplo do documento 14, §6.6, usa `cycle("10ms")`, que fica abaixo do mínimo de tier 2 (§19, `J-E0854`). Com 500 ms, um clique mais curto do que o período perde-se, porque `clicked` = `rose(pressed)` é avaliado no latch de quem o consome (14 A7). Para botões em Python ou TS, a forma certa é uma tarefa `task.on("<porta>")` sobre uma porta ligada ao sinal do botão (cada mudança é entregue), ou uma implementação ativa em Rust.

### 14.4 Classes locais

```python
@local_class
class Zone:
    temp = Input(float, unit="°C")
    occupied = Input(bool)
    setpoint = Output(float, unit="°C")
    damper = Output(float, unit="%")

@app(name="floor_ctl", version="0.1.0")
class FloorCtl(App):
    zones = devices(Zone, 1, 64)
```

```ts
const Zone = localClass({
  inputs: { temp: input.f64("°C"), occupied: input.bool() },
  outputs: { setpoint: output.f64("°C"), damper: output.f64("%") },
});
export default app({ name: "floor_ctl", version: "0.1.0", devices: { zones: many(Zone, { min: 1, max: 64 }) }, /* … */ });
```

Liga-se com `each`/`map` (14 §6.5). Uma classe local não tem métodos nem consultas de contrato: o comportamento é código da app, que em Python pode ser um método da classe local e em TS uma função.

### 14.5 Implementar uma classe (instância virtual)

```python
from joker_classes.std import Pump

@implements(Pump, capabilities=["hour_counter"])
@app(name="pump_ctl", version="1.2.0")
class PumpCtl(App):
    cmd: Pump.Commands                    # anotação para os tipos; o SDK preenche-a
    state: Pump.State
    contactor = Output(bool)
    feedback = Input(bool)
    thermal_trip = Input(bool)
    min_off = Param(float, unit="s", default=180.0)
    min_on = Param(float, unit="s", default=60.0)

    @on_action("reset")
    async def reset(self, args: None) -> None:
        ...                                # rearme; uma exceção vira ActionError(device) para quem chamou
```

- `self.cmd` / `io.cmd` tem os `out` da classe como entradas (comandos recebidos, já arbitrados e com envelope, 14 §7.2); `self.state` / `io.state` tem os `in` da classe como saídas publicadas; os `event` emitem-se com `self.state.<evento>.emit(r)`.
- `@on_action("<ação>")` / `onAction: { <ação>: async (io, args, cx) => … }` recebe os argumentos já descodificados e, opcionalmente, um `cx` (`ActionCx`: `key`, `caller`, `deadline`, `progress(fraction, message)` sobre `report-progress`, `is_cancelled()`/`isCancelled()` sobre `is-cancelled`), e devolve o resultado (ou `None`). Mapeia para o export `on-action` (ref/04 §2.6.1). Para terminar uma ação cancelada, lança `ActionError.cancelled()` (`J-E0308`). Uma ação da classe sem handler é `J-E0813` no build.
- `Pump.Commands` e `Pump.State` são gerados pelo `jkc`. Em TS, `io.cmd` e `io.state` têm o tipo certo sem anotação.

### 14.6 Membros de classe (handles aninhados)

Um membro de classe (documento 17, §8) é uma **propriedade** do handle com o handle da classe do membro, como em Rust (ref/04 §4.8.6). Os nomes do utilizador ficam em `snake_case` nas duas linguagens.

```ts
// gerado a partir de classes/local/bench_io.jkc (excerto)
interface BenchIoCycleCore extends ClassHandleCycle<BenchIoState> {
  readonly status: LedCycle<never>;
  readonly boot: ButtonCycle<never>;
  readonly knob: AnalogInCycle<never>;
  rssi: Member<bigint>; uptime: Member<number>; online: Member<boolean>;
}
// um membro coleção: readonly pumps: Many<PumpCycle<never>>; um @optional: readonly grid?: SgReadyCycle<never>
```

```python
# gerado a partir de classes/local/bench_io.jkc (excerto)
class BenchIo(ClassHandle[BenchIoState]):
    CLASS: ClassVar[str] = "local/bench_io@1"
    status: Led
    boot: Button
    knob: AnalogIn
    rssi: Member[int]
    uptime: Member[float]
    online: Member[bool]
# um membro coleção: pumps: Many[Pump]; um @optional: grid: SgReady | None
```

| Forma | TypeScript | Python |
|---|---|---|
| consulta de um membro fixo | `io.esp.boot.clicked()` | `self.esp.boot.clicked()` |
| coleção | `for (const p of io.station.pumps) …` | `for p in self.station.pumps: …` |
| método de um membro | `io.hp.compressor.stop()` | `self.hp.compressor.stop()` |
| membro `@optional` | `io.hp.grid?.boost()` | `if self.hp.grid is not None: self.hp.grid.boost()` |

A semântica é a do §14.2 e da ref/04 §4.8.6: as escritas são do escritor da app, e numa instância virtual os membros de classe só se leem (o método é descartado com `J-E0507`, documento 16, P6). O `health()` do handle composto é o da árvore (17 §4.5), com os mesmos vetores de teste de `conformance/sdk/health.json`.

---

## 15. Codificações no guest

### 15.1 Imagem e propostas

O SDK descodifica a imagem empacotada do `on-cycle` e escreve o buffer de propostas com o layout da ref/04 §3: cabeçalho de 64 B, amostras e resultados de 24 B, secção larga; registos `WRITE`, `METHOD`, `PUBLISH`, `COUNTER_ADD`, `EMIT` e `NOTE`. Em Python, com `struct.unpack_from`/`pack_into` sobre um `memoryview`, sem cópias por leitura; em TS, com `DataView` sobre o `Uint8Array`. Os valores de `string`, `bytes` e `Internal` até 64 B seguem no próprio registo, com a flag `INLINE` (ref/04 §3.3); `cycle-io.read` só é preciso para os carimbos `ts_src`/`ts_hw` (flags `HAS_TS_*` da amostra) e para acesso dinâmico por nome.

**Handles.** O handle da porta *i* é a sua posição na tabela de portas, construída pela ordem da lista de portas do `joker:meta` (ref/04 §2.6.2). Os SDKs de tier 2 emitem as portas **ordenadas por nome** (bytes UTF-8), e não pela ordem de declaração: assim a ordem não depende de detalhes do interpretador, e o mesmo código dá o mesmo `joker:meta` byte a byte. O Rust usa a ordem dos campos da struct; o host não distingue.

### 15.2 Registos

- CBOR determinístico (RFC 8949, §4.2.1), com a forma de D2 (ref/00 §4.1, ref/04 §4.10): um mapa com **chaves inteiras**, a posição do campo no esquema do `joker:meta` a contar de 0; inteiros na forma mais curta; `f64` na forma mais curta que preserve o valor; um campo opcional a `None`/`null` omite-se. `Plan(target_soc=0.62, limit_kw=400.0, valid_until=…)` é `{0: 0.62, 1: 400.0, 2: …}`.
- Cada SDK tem o seu codificador (sem dependências), e todos passam os vetores de `conformance/sdk/cbor.json`: os mesmos registos dão os mesmos bytes em Rust, TS e Python.
- Limites (ref/00 §10): `string` ≤ 1 KiB, listas ≤ 64, `Internal` ≤ 64 B codificado. Fora disso, `LimitError` na codificação (`J-E0805`, `J-E0804`).
- As interfaces de texto (API, `jk`, logs do SDK) mostram os campos pelo nome, a partir do esquema.

---

## 16. Mapeamento para o mundo `joker:runtime`

Cada função do mundo (`wit/joker-runtime.wit`, ref/04 §2) tem a sua forma nas três linguagens. "Imagem" e "buffer" querem dizer que o SDK não chama o host: usa a imagem do `on-cycle` ou escreve um registo no buffer de propostas (ref/04 §3). Os itens `@unstable` (1.1) existem nos tipos de tier 2, com o comportamento do SDK Rust (§7, ref/04 §2.9): recusados em `prod` (`J-E0003`), e em `dev` levantam `UnavailableError` com o `J-E0601` dos stubs.

**`cycle-io` (instância de ciclo)**

| WIT | Rust (ref/04) | Python | TypeScript | Caminho |
|---|---|---|---|---|
| `read` | `get`, `get_uncertain`, `get_or`, `sample` | `get()`, `get_uncertain()`, `get_or()`, `sample()` | `get()`, `getUncertain()`, `getOr()`, `sample()`, `tryGet()` | imagem; `read` para `ts_src`/`ts_hw` |
| `write` | `CycleOutput::set` | `OutputHandle.set` | `OutputCycle.set` | buffer (`WRITE`) |
| `relinquish` | `relinquish` | `relinquish()` | `relinquish()` | buffer (`WRITE` com `RELINQUISH`) |
| `pending` | `pending` | `pending()` | `pending()` | SDK (buffer local) |
| `last-outcome` | `last_outcome`, `mine`, `winner` | `last_outcome()` | `lastOutcome()` | imagem (secção de resultados) |
| `param` | `ParamHandle::get` | `ParamHandle.get()` | `ParamHandle.get()` | imagem |
| `call` | método da classe (`p.start()`) | `p.start()` | `p.start()` | buffer (`METHOD`) |
| `publish` | `PublishedHandle::set`, `AlarmHandle::set/raise/clear`, `CycleInternal::set` (escritor de ciclo), `io.state.<m>.set` | `Published.set`, `Alarm.set/raise_/clear`, `Internal.set`, `self.state.<m>.set` | `set`, `raise`/`clear`, `set`, `io.state.<m>.set` | buffer (`PUBLISH`) |
| `counter-add` | `CounterHandle::inc/add` | `Counter.add(n)` | `CounterHandle.add(n)` | buffer (`COUNTER_ADD`) |
| `emit` | `EventOutputHandle::emit`, `io.events.<e>.emit` | `EventOutput.emit`, `self.state.<e>.emit` | `emit` | buffer (`EMIT`) |
| `note-substituted` | `get_or` | `get_or()` | `getOr()` | buffer (`NOTE`) |
| `mailbox-send` | `MailboxEnd::try_send` | `Mailbox.try_send` | `MailboxSend.trySend` | host |
| `mailbox-recv` | `MailboxEnd::try_recv` | `Mailbox.try_recv`, `drain` | `MailboxRecvCycle.tryRecv`, `drain` | host |
| `now` | `CycleIo::now` | `self.now()` (e `time.*`) | `io.now()` (e `Date.now()`) | imagem (cabeçalho); `wasi:clocks` servido com o latch |
| `dt-ns` | `CycleIo::dt` | `self.dt()` | `io.dt()` | imagem (cabeçalho) |
| `random-u64` | `CycleIo::random` | `random` (semeado) | `Math.random()` (semeado) | host, via `wasi:random` |
| `write-at` (1.1) | `write_at` (`unstable-scheduled-writes`) | `set_at` | `setAt` | buffer (`WRITE` com `AT`) |

**`async-io` (instância async)**

| WIT | Rust (ref/04) | Python | TypeScript |
|---|---|---|---|
| `read` | `TaskInput::get`, `sample`, … | `get()`, `sample()`, … | `get()`, `sample()`, … |
| `snapshot` | `TaskIo::snapshot` | `self.snapshot(*ports)` | `io.snapshot([...])` |
| `set` | `TaskOutput::set` | `OutputHandle.set` | `OutputTask.set` |
| `write` | `TaskOutput::write(v).await` | `await write(v)` | `await write(v)` |
| `relinquish` | `relinquish` | `relinquish()` | `relinquish()` |
| `last-outcome` | `last_outcome` | `last_outcome()` | `lastOutcome()` |
| `param` | `ParamHandle::get` | `ParamHandle.get()` | `ParamHandle.get()` |
| `call` | método da classe `.await` → `MethodOutcome` | `await p.start()` → `list[WriteOutcome]` | `await p.start()` → `WriteOutcome[]` |
| `invoke` | `ActionPort::invoke(args).await`, `p.reset().await` | `await invoke(args, key=)` | `await invoke(args, { key })` |
| `start-action`, `action-call.*` | `ActionCall::start`, `ActionHandle::{progress, cancel, finish}` | `invoke_long()`, `LongAction.{progress, cancel, finish}` | `invokeLong()`, `LongAction.{progress, cancel, finish}` |
| `publish` | `PublishedHandle::set`, `AlarmHandle::set`, `TaskInternal::set` (escritor async) | `set`, `Alarm.set`, `Internal.set` | `set`, `set`, `set` |
| `counter-add` | `CounterHandle::add` | `Counter.add` | `CounterHandle.add` |
| `emit` | `EventOutputHandle::emit` | `emit` | `emit` |
| `note-substituted` | `get_or` | `get_or()` | `getOr()` |
| `subscribe` | `TaskInput::subscribe` | `self.subscribe(*ports)` | `io.subscribe([...])` |
| `events` | `EventInputHandle::next/stream` | `EventInputHandle.events()` | `EventInputHandle.events()` |
| `mailbox-send`, `mailbox-recv` | `MailboxEnd::try_send/try_recv` | `try_send`, `try_recv`, `drain` | `trySend`, `tryRecv`, `drain` |
| `mailbox-next` | `MailboxEnd::recv().await` | `await recv()` | `await recv()` |
| `history` | `history`, `history_agg` | `await history(…, step=, agg=)` | `await history({ …, step, agg })` |
| `now` | `TaskIo::now` | `self.now()` | `io.now()` |
| `sleep-until` | `TaskIo::sleep`, `sleep_until` | `await self.sleep()`, `sleep_until()` (e `asyncio.sleep`) | `await io.sleep()`, `sleepUntil()` (e `setTimeout`) |
| `random-u64` | `TaskIo::random` | `random` | `Math.random()` |
| `report-progress`, `is-cancelled` | `ActionCx::progress`, `is_cancelled` | `cx.progress()`, `cx.is_cancelled()` | `cx.progress()`, `cx.isCancelled()` |
| `write-at` (1.1) | `write_at` | `await write_at()` | `await writeAt()` |
| `propose` (1.1) | `propose` (`unstable-intents`) | `await propose(v, reason=, ttl=)` | `await propose(v, { reason, ttl })` |

**`kv`, `secrets`, `log`**

| WIT | Rust (ref/04) | Python | TypeScript |
|---|---|---|---|
| `kv.get` | `Kv::get`, `get_record` | `kv.get`, `get_record` | `kv.get`, `getRecord` |
| `kv.set` | `Kv::set`, `set_record` | `await kv.set`, `set_record` | `await kv.set`, `setRecord` |
| `kv.delete` | `Kv::delete` | `await kv.delete` | `await kv.delete` |
| `kv.keys` | `Kv::keys` | `kv.keys(prefix)` | `kv.keys(prefix)` |
| `kv.commit` | `Kv::batch().….commit().await` | `await kv.batch().….commit()` | `await kv.batch().….commit()` |
| `secrets.reveal` | `TaskIo::secret(name)` → `Secret` | `await self.secrets.get(name)` → `SecretStr` | `await io.secrets.get(name)` → `SecretString` |
| `log.log` | `log::info!(…)` e as outras macros | `log.info(…)`, `logging`, `print` | `log.info(…)`, `console.*` |
| `log.enabled` | `joker::log::enabled(Level)` | `log.enabled(level)` | `log.enabled(level)` |

**`guest` (exports)**

| WIT | Rust (ref/04) | Python | TypeScript |
|---|---|---|---|
| `init` | gerado pelo `#[joker::app]` | SDK (guarda parâmetros, semeia `random`, resolve handles) | idem |
| `on-params` | `#[joker::on_params]` | `@on_params` | `onParams` |
| `on-cycle` | `#[joker::cycle]` | `@cycle` | `cycles: { … }` |
| `run-task` | `#[joker::task]` | `@task.*` | `tasks: { … }` |
| `on-action` | `#[joker::on_action]` | `@on_action` | `onAction` |
| `export-state`, `migrate` | `#[joker::state(persist)]`, `#[joker::migrate]` | `@state(persist=True)`, `@migrate` | `cycle({ persist: true })`, `migrate` |

**Reservados (1.1):** `ml.infer` e o recurso `tensor` (`dtype`, `shape`, `data`) → `Model.infer`, `Tensor`; `frames.next` e o recurso `frame` (`meta`, `view`) → `StreamInput`; `blocks.next` → `BlockInput`: nos tipos; na v1, `J-E0003` em `prod` e `UnavailableError` (`J-E0601`) em `dev` (os nomes Rust são os da ref/04 §4.15). **WASI:** `wasi:clocks` (latch no ciclo, real em async), `wasi:random` (o gerador determinista), `wasi:http/outgoing-handler` (`fetch`, `joker_sdk.http`, com `net.allow`), como na ref/04 §2.7.

## 17. Testes

O harness é a crate `joker-test` do SDK Rust (04 §3.7), embrulhada: **PyO3** no pacote `joker-testing` e **napi-rs** no `@joker/test`, com rodas e binários pré-compilados para Linux (x86_64, aarch64), macOS (aarch64, x86_64) e Windows (x86_64). Corre o componente real (`.wasm`) no wasmtime, com o relógio virtual e o `joker-core` verdadeiro: o mesmo harness, a mesma semântica, nas três linguagens.

### 17.1 API

```python
class Harness(Generic[AppT]):
    @classmethod
    def for_app(cls, app: type[AppT] | None = None, *, wasm: str | os.PathLike[str] | None = None) -> Harness[AppT]: ...
    # configuração (encadeável)
    def group(self, task: str, period: DurationLike) -> Self: ...
    def param(self, name: str, value: Any) -> Self: ...
    def input(self, name: str, value: Series | Any) -> Self: ...
    def internal(self, name: str, value: Any) -> Self: ...
    def history(self, name: str, series: Series, *, before: DurationLike) -> Self: ...   # historian pré-carregado
    def instance(self, port: str, name: str, *, capabilities: Sequence[str] = (), sim: bool = True) -> Self: ...
    def member(self, instance: str, member: str, value: Series | Any) -> Self: ...
    def action(self, target: str, handler: Callable[[Any], Any] | BaseException) -> Self: ...
    def envelope(self, output: str, *, range: tuple[Any, Any] | None = None, rate: str | None = None,
                 safe: Any = None, interlock: str | None = None) -> Self: ...
    def clock(self, *, start: str = "2026-01-01T00:00:00Z", quality: str = "synced") -> Self: ...
    def seed(self, seed: int) -> Self: ...
    # execução, em tempo virtual
    def run_for(self, d: DurationLike) -> None: ...
    def step(self, ticks: int = 1) -> None: ...
    def run_task(self, task: str) -> None: ...          # força uma execução de uma tarefa async
    def now(self) -> Instant: ...
    # observação
    def output(self, name: str) -> OutputTrace: ...     # propostas, resultados e valores aplicados
    def input_series(self, name: str) -> Trace: ...
    def events(self, name: str) -> list[tuple[Instant, Any]]: ...
    def alarm(self, name: str) -> Trace: ...
    def traps(self) -> list[Trap]: ...
    def task_errors(self) -> list[TaskError]: ...
    def logs(self, *, level: str = "debug") -> list[LogRecord]: ...
```

```ts
export declare class Harness {
  static forApp(app: AppComponent, o?: { wasm?: string }): Harness;
  group(task: string, period: DurationLike): this;
  param(name: string, value: unknown): this;
  input(name: string, value: Series | unknown): this;
  internal(name: string, value: unknown): this;
  history(name: string, s: Series, o: { before: DurationLike }): this;
  instance(port: string, name: string, o?: { capabilities?: string[]; sim?: boolean }): this;
  member(instance: string, member: string, value: Series | unknown): this;
  action(target: string, handler: ((args: unknown) => unknown) | Error): this;
  envelope(output: string, o: { range?: [unknown, unknown]; rate?: string; safe?: unknown; interlock?: string }): this;
  clock(o: { start?: string; quality?: "synced" | "holdover" | "unsynced" }): this;
  seed(seed: number): this;
  runFor(d: DurationLike): void;
  step(ticks?: number): void;
  runTask(task: string): void;
  now(): Instant;
  output(name: string): OutputTrace;
  inputSeries(name: string): Trace;
  events(name: string): Array<[Instant, unknown]>;
  alarm(name: string): Trace;
  traps(): Trap[];
  taskErrors(): TaskError[];
  logs(o?: { level?: string }): LogRecord[];
}
```

- **Séries:** `Series.constant(v)`, `Series.step(before, after, at)`, `Series.ramp(a, b, over)` (fica em `b` depois de `over`), `Series.linear(a, b, over)` (continua a reta), `Series.sine(mean, amp, period)`, `Series.noise(mean, std, seed=)`, `Series.replay(path, column)` (Parquet), e `.with_quality_at(t, q)`/`.withQualityAt(t, q)`, `.with_quality(start, end, q)`. As mesmas do `joker-test`.
- **Traços:** `.values()`, `.last()`, `.when(cond)`, `.after(t)`, `.before(t)`, `.all(pred)`, `.any(pred)`, `.no_new_writes()`/`.noNewWrites()`, `.outcomes()`, `.lt(x)`/`.gt(x)`/`.eq(x)` (para condições).
- **Envelope:** sem `envelope()`, cada proposta é `accepted`. Com `envelope()`, o harness usa o envelope real do `joker-core` com essa configuração, e os resultados são os de produção.
- **Instâncias de classe:** `instance(port, name, sim=True)` liga uma instância ao simulador de classe (14 A13); com `sim=False`, os membros vêm de `member()`.
- **Tarefas async** correm no mesmo relógio virtual; `history()` é servido pelo que o harness já gerou mais o pré-carregado.

### 17.2 pytest

```python
# tests/test_peak_shaver.py
from joker_testing import Harness, Series, Quality
from peak_shaver import PeakShaver, Plan

def test_nunca_descarrega_abaixo_da_reserva() -> None:
    h = (Harness.for_app(PeakShaver)
         .group("control", "200ms")
         .param("limit", 400.0).param("reserve", 10.0)
         .input("grid_power", Series.ramp(300.0, 700.0, "10m"))
         .input("soc", Series.linear(0.30, 0.05, "10m"))
         .internal("plan", Plan(target_soc=0.5, limit_kw=400.0, valid_until=2**62)))
    h.run_for("10m")                                    # relógio virtual: corre em milissegundos
    soc = h.input_series("soc")
    assert h.output("battery").when(soc.lt(0.1)).all(lambda v: v == 0.0)
    assert h.traps() == []

def test_qualidade_ma_nao_escreve() -> None:
    h = (Harness.for_app(PeakShaver)
         .input("grid_power", Series.constant(500.0).with_quality_at("30s", Quality.bad("comm_failure")))
         .input("soc", 0.5))
    h.run_for("60s")
    assert h.output("battery").after("30s").no_new_writes()
```

O plugin (`pytest11`, ativado ao instalar `joker-testing`) faz o `jk build` da app uma vez por sessão, só se as fontes mudaram, e `Harness.for_app` usa esse `.wasm`. `jk test` corre o `pytest` das apps Python.

### 17.3 vitest e `node:test`

```ts
// test/peak_shaver.test.ts
import { describe, expect, it } from "vitest";
import { Harness, Series, Quality } from "@joker/test";
import peakShaver from "../src/app.js";

describe("peak_shaver", () => {
  it("nunca descarrega abaixo da reserva", () => {
    const h = Harness.forApp(peakShaver)
      .group("control", "200ms")
      .param("limit", 400).param("reserve", 10)
      .input("grid_power", Series.ramp(300, 700, "10m"))
      .input("soc", Series.linear(0.30, 0.05, "10m"))
      .internal("plan", { target_soc: 0.5, limit_kw: 400, valid_until: 2n ** 62n });
    h.runFor("10m");
    const soc = h.inputSeries("soc");
    expect(h.output("battery").when(soc.lt(0.1)).all((v) => v === 0)).toBe(true);
    expect(h.traps()).toEqual([]);
  });
});
```

`vitest.config.ts` com `globalSetup: ["@joker/test/setup"]` faz o build uma vez. Com `node:test`, `import "@joker/test/register"` faz o mesmo no primeiro `Harness.forApp`. A API não depende do *framework*: funciona também com Jest.

### 17.4 Modo nativo

`import joker_sdk` num CPython normal, e `import "@joker/sdk"` num Node normal, funcionam em **modo nativo**: as declarações e as funções puras da app podem ser testadas diretamente (`Plan.from_peak(…)`), sem harness. Qualquer chamada ao host levanta `J-E0818`. É também o modo que o `jk build` usa para extrair os metadados (§18.4).

---

## 18. Empacotamento e build

### 18.1 Python: `pyproject.toml`

```toml
[project]
name = "peak-shaver"
version = "1.4.2"
requires-python = ">=3.12"
dependencies = ["joker-sdk ~= 1.2"]

[project.optional-dependencies]
test = ["joker-testing ~= 1.2", "pytest >= 8"]

[tool.joker]
app = "peak_shaver:PeakShaver"
include = ["peak_shaver/data/*.json"]
```

### 18.2 TypeScript: `package.json`

```json
{
  "name": "peak-shaver",
  "version": "1.4.2",
  "type": "module",
  "private": true,
  "joker": { "app": "src/app.ts" },
  "dependencies": { "@joker/sdk": "^1.0.0" },
  "devDependencies": { "@joker/test": "^1.0.0", "typescript": "^5.6.0", "vitest": "^3.0.0" },
  "scripts": { "build": "jk build", "test": "vitest run" }
}
```

### 18.3 Chaves

As chaves `[tool.joker]` e `"joker"` são as mesmas. São ficheiros escritos por pessoas, e por isso estritos: uma chave desconhecida é `J0140`; as chaves `x_*` são ignoradas (ref/00 §1).

| Chave | Tipo | Obrigatória | Omissão | Validação |
|---|---|---|---|---|
| `app` | Python: `"módulo:Classe"`; TS: caminho do ficheiro de entrada | sim | — | tem de ter exatamente uma app (`J-E0850`) |
| `world` | `"joker:runtime@X.Y.Z"` | não | o mundo do SDK | tem de ser um mundo que o SDK instalado sabe gerar (`J-E0856`) |
| `include` | lista de globs relativos ao pacote | não | `[]` | ficheiros de dados embutidos só de leitura; total ≤ 4 MiB (`J-E0860`) |

**Não há mais chaves, de propósito.** O nome, a versão, as portas, as tarefas, as unidades e os alarmes estão no código (04 §1). A memória, o grupo, o nível no árbitro e a rede são do site (`[apps.<a>]`, 03 §8). A `version` do código é opcional: sem ela, vale a do `[project]`/`"version"`; com as duas, têm de ser iguais (`J-E0852`). O `name` do código é obrigatório (é o nome da app no manifesto, em `snake_case`), e o do pacote é livre (`peak-shaver`).

### 18.4 O que o `jk build` faz

1. **Deteta a linguagem:** `Cargo.toml` → Rust (ref/04); `package.json` com `"joker"` → TS; `pyproject.toml` com `[tool.joker]` → Python. Mais do que um na mesma pasta é `J-E0850`.
2. **Verifica a toolchain:** TS exige Node ≥ 20 e o `@joker/sdk` em `node_modules`; Python exige o interpretador do `requires-python` e o `joker-sdk` no ambiente ativo (ou em `.venv`), que fixa a versão do `componentize-py`. Em falta: `J-E0861`, com o comando que resolve.
3. **Executa a declaração em modo nativo** (§17.4): importa o ponto de entrada, recolhe o registo, valida (§5.5), produz o `joker:meta`, e corre os *lints* estáticos: `J-E0859` (API async numa tarefa de ciclo), `J-E0858` (global do módulo escrita numa tarefa) e `J-E0863` (captura genérica de exceções à volta de leituras numa tarefa de ciclo).
4. **Empacota:** TS com o esbuild (ESM, `es2022`, um só ficheiro, com o `@joker/sdk` incluído; imports `node:*` são `J-E0853`); Python com o pacote de entrada e as dependências do ambiente, que têm de ser Python puro ou ter rodas `wasm32-wasi` (`J-E0853`, com a lista das que falham).
5. **Componentiza com pré-inicialização:** ComponentizeJS (StarlingMonkey, com weval) ou componentize-py (CPython, com Wizer), contra o `wit/` do mundo pedido. O código de topo volta a correr dentro do Wasm, e o SDK compara o digest do registo com o do passo 3: diferença é `J-E0862` (a declaração depende do ambiente).
6. **Injeta** a secção `joker:meta` e escreve `dist/<app>.wasm`. Com `--target`, pré-compila o `.cwasm` (01 §9).
7. **Verifica os limites** de tamanho do §19 (`J-E0860`).

O build é reprodutível: as mesmas fontes, as mesmas dependências (fixadas pelo lock do ecossistema: `package-lock.json`, `uv.lock` ou `pylock.toml`), a mesma versão do SDK e da toolchain dão os mesmos bytes. Sem lock do ecossistema, o `jk build` avisa (`J-E0864`), porque o digest do lock do Joker deixa de ser reprodutível a partir do Git.

`jk app new <nome> --lang ts|py` gera o `package.json` ou o `pyproject.toml`, o ponto de entrada, um teste, o lock do ecossistema e, em TS, o `tsconfig.json` (`strict`, `noUncheckedIndexedAccess`) e a regra de lint `no-floating-promises`.

---

## 19. Limites de tier 2

**Tier 2 corre no ciclo e em async**, com um período mínimo de ciclo. Proibir o ciclo obrigaria a escrever em Rust controlos lentos que o TS e o Python fazem bem (HVAC a 1 s, níveis de tanques a 1 s), e empurraria as pessoas para tarefas `every("1s")` em async, que não têm latch, nem determinismo, nem admissão de tempo real. O que torna o ciclo aceitável é que as pausas de GC acontecem **dentro** do tick, são medidas pelo `jk bench` e entram na admissão (M23).

| | Rust (04 §4.3) | TypeScript | Python | Onde se aplica |
|---|---|---|---|---|
| período mínimo de uma tarefa de ciclo | 1 ms | **100 ms** | **500 ms** | `jk check` e `jk build`: `J-E0854` (erro em `prod`, aviso em `dev`) |
| `wcet` por omissão | declarado | 20% do período | 20% do período | até haver medição no lock |
| `wcet` máximo | admissão | 50% do período | 50% do período | `J-E0854` |
| admissão com medição | p99,9 | **máximo** medido | **máximo** medido | o `jk bench` corre ≥ 10 000 ticks e grava `p999` e `max` no `[wcet]` do lock; a admissão de tier 2 usa o `max` (D15, ref/02 §2.1.13) |
| deadline por epoch | `max(2 × wcet, 200 µs)` (01 §4) | idem | idem | host |
| `memory.max` por omissão (ciclo / async) | 16 / 64 MiB (01 §9) | **64 / 64 MiB** | **128 / 128 MiB** | o SDK põe a sugestão no `joker:meta`; `[apps.<a>] resources.memory` do manifesto vence; entra no `J0380` |
| tamanho do componente (`.wasm`) | 0,1–2 MB | alvo 8–12 MB, máximo 32 MB | alvo 15–30 MB, máximo 64 MB | `J-E0860` |
| arranque a frio (do `.cwasm` em cache até ao fim do `init`), p95 num CM4 | < 10 ms | **≤ 100 ms** | **≤ 300 ms** | alvo; a pré-inicialização e as memórias *copy-on-write* do pooling tornam-no possível |
| AOT no dispositivo (sem `.cwasm` pré-compilado), CM4 | segundos | ≤ 60 s | ≤ 180 s | alvo; por isso o `jk build --target` é o caminho normal |
| sobrecusto do `on-cycle` com uma tarefa vazia e 20 portas | ≤ 5 µs | ≤ 200 µs | ≤ 1 ms | alvo |
| pausa de GC p99,9 com 32 MiB de heap vivo | — | ≤ 5 ms | ≤ 20 ms | alvo; medida no `jk bench` |
| hot-swap | 0 ticks perdidos | 0 ticks perdidos | 0 ticks perdidos | a instância nova é criada ao lado (01 §9) |
| tipos e unidades | compilação | tipos na compilação (`tsc`); unidades no `jk build` | tipos no mypy/pyright; unidades no `jk build` | `J0109`, `J0107` no `jk check` como em Rust |
| arquiteturas | todas (01 §22) | x86_64, aarch64, riscv64 (Cranelift) | idem | armv7 (Pulley) é `J-E0855`: um interpretador de JS ou Python sobre um interpretador de Wasm fica 50–100× mais lento |
| Joker Micro | sim (nativo) | **não** | **não** | `jk build --target mcu:…` falha com `J0720` |

- Os números "alvo" são estimativas a validar quando cada SDK existir (TS na Fase 3, Python na 1.2), como os do documento 01, §21. Os mínimos de período, os máximos de WCET e de tamanho, e as arquiteturas são **regras**, aplicadas pelo `jk check`.
- Uma tarefa de tier 2 pode partilhar um grupo com tarefas Rust, se o período do grupo respeitar o mínimo. A pausa de GC atrasa as tarefas seguintes do mesmo tick, e a admissão conta-a. Para malhas rápidas, a regra do documento 04 continua: Rust.
- Se o *async* do Component Model (WASI 0.3) não estiver estável no ComponentizeJS ou no componentize-py quando o SDK sair, vale o **plano B** do §29, D9: o mundo `app-blocking` da ref/04 §2.11.

---

## 20. Paridade: a mesma app em três linguagens

As duas apps abaixo são a prova de que a API fecha. O Rust segue o documento 04 e é **ilustrativo** (a sintaxe normativa é a do ref/04); o TS e o Python são normativos. Com uma exceção deliberada no §20.1 (o período e o WCET, por causa do §19), as três versões têm as mesmas portas, os mesmos nomes, os mesmos resultados de escrita e a mesma semântica de qualidade.

### 20.1 `peak_shaver` (documento 04, §3)

Versão da v1.0, sem ML: o plano é calculado a partir do historian. A porta de modelo e o `infer` da 1.1 estão no fim da secção. Duas correções face ao documento 04 (§33, A3 e A4): a porta chama-se `grid_power`, como no `bind` do documento 03, §8, e, com o plano expirado ou ainda inexistente, o limite é o do parâmetro (com o `Plan::default()` do documento 04, o limite seria 0 kW e a bateria descarregaria para cobrir toda a importação da rede).

**Rust** (ilustrativo):

```rust
use joker::prelude::*;

#[derive(Record, Clone, Copy, Default)]
pub struct Plan { target_soc: f64, limit_kw: f64, valid_until: UtcSeconds }

impl Plan {
    fn from_peak(peak_kw: f64, limit_kw: f64, now: Instant) -> Self {
        let excess = (peak_kw - limit_kw).max(0.0);
        let target = (0.3 + excess / 500.0).clamp(0.3, 0.95);
        Plan { target_soc: target, limit_kw, valid_until: now.utc_seconds() + 30 * 60 }
    }
}

#[derive(joker::Event)]
pub struct LowSoc { soc: f64 }

#[joker::app(name = "peak_shaver", version = "1.4.2")]
pub struct PeakShaver {
    #[input(unit = "kW", quantity = "active_power")] grid_power: Input<f64>,
    #[input(unit = "1")]                               soc: Input<f64>,
    #[output(unit = "kW", quantity = "active_power")] battery: Output<f64>,
    #[param(unit = "kW", range = "0..5000", default = 400.0)] limit: Param<f64>,
    #[param(unit = "%", range = "0..50", default = 10.0)]     reserve: Param<f64>,
    #[internal]                                        plan: Internal<Plan>,
    #[alarm(severity = "medium", message = "plano expirado")] plan_stale: Alarm,
    #[output(event)]                                   low_soc: EventOutput<LowSoc>,
}

#[derive(Default)]
struct Ctl { integral: f64 }

#[joker::cycle(period = "200ms", wcet = "40us")]
fn control(io: &mut CycleIo<PeakShaver>, st: &mut Ctl) -> Result<()> {
    let grid = io.grid_power.get()?;
    let soc = io.soc.get()?;
    let plan = io.plan.get_or_default();
    let stale = plan.valid_until < io.now().utc_seconds();
    io.plan_stale.set(stale);
    let limit = if stale { io.limit.get() } else { plan.limit_kw.min(io.limit.get()) };
    let excess = grid - limit;
    st.integral = (st.integral + excess * io.dt().as_secs_f64()).clamp(-50.0, 50.0);
    let floor = io.reserve.get() / 100.0;
    io.battery.set(if soc <= floor { 0.0 } else { (excess + 0.1 * st.integral).max(0.0) });
    Ok(())
}

#[joker::task(every = "15m", jitter = "30s")]
async fn replan(io: &TaskIo<PeakShaver>) -> Result<()> {
    let now = io.now();
    let hist = io.grid_power.history(now - 24.h(), now, 15.mins()).await?;
    let peak = hist.iter().filter_map(|s| s.good().copied()).fold(0.0_f64, f64::max);
    io.plan.set(Plan::from_peak(peak, io.limit.get(), now));
    Ok(())
}

#[joker::task(on = "soc")]
async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()> {
    if let Ok(v) = s.get() {
        if v < 0.05 { io.low_soc.emit(LowSoc { soc: v }); }
    }
    Ok(())
}
```

**TypeScript** (`src/app.ts`):

```ts
import { alarm, app, cycle, input, internal, output, param, record, t, task, type Infer, type Instant } from "@joker/sdk";

const Plan = record({ target_soc: t.f64(), limit_kw: t.f64({ unit: "kW" }), valid_until: t.utcSeconds() });
type Plan = Infer<typeof Plan>;
const LowSoc = record({ soc: t.f64({ unit: "1" }) });

function planFromPeak(peakKw: number, limitKw: number, now: Instant): Plan {
  const excess = Math.max(peakKw - limitKw, 0);
  const target = Math.min(Math.max(0.3 + excess / 500, 0.3), 0.95);
  return { target_soc: target, limit_kw: limitKw, valid_until: now.utcSeconds() + 1800n };
}

export default app({
  name: "peak_shaver",
  version: "1.4.2",
  inputs: {
    grid_power: input.f64({ unit: "kW", quantity: "active_power" }),
    soc: input.f64({ unit: "1" }),
  },
  outputs: {
    battery: output.f64({ unit: "kW", quantity: "active_power" }),
    low_soc: output.event(LowSoc),
  },
  params: {
    limit: param.f64({ unit: "kW", range: [0, 5000], default: 400 }),
    reserve: param.f64({ unit: "%", range: [0, 50], default: 10 }),
  },
  internals: { plan: internal(Plan) },
  alarms: { plan_stale: alarm({ severity: "medium", message: "plano expirado" }) },

  cycles: {
    control: cycle({ period: "200ms", wcet: "20ms", state: () => ({ integral: 0 }) }, (io, st) => {
      const grid = io.grid_power.get();          // QualityError → o tick desta tarefa termina aqui
      const soc = io.soc.get();
      const plan = io.plan.getOrDefault();
      const stale = plan.valid_until < io.now().utcSeconds();
      io.plan_stale.set(stale);
      const limit = stale ? io.limit.get() : Math.min(plan.limit_kw, io.limit.get());
      const excess = grid - limit;
      st.integral = Math.min(Math.max(st.integral + excess * io.dt().seconds, -50), 50);
      const floor = io.reserve.get() / 100;      // % → fração, como o soc
      io.battery.set(soc <= floor ? 0 : Math.max(excess + 0.1 * st.integral, 0));
    }),
  },

  tasks: {
    replan: task.every("15m", { jitter: "30s" }, async (io) => {
      const now = io.now();
      const hist = await io.grid_power.history({ start: now.minus("24h"), end: now, step: "15m" });
      const peak = Math.max(0, ...hist.values());
      io.plan.set(planFromPeak(peak, io.limit.get(), now));
    }),
    watch_soc: task.on("soc", async (io, s) => {
      if (s.isGood() && s.value < 0.05) io.low_soc.emit({ soc: s.value });
    }),
  },
});
```

**Python** (`peak_shaver/__init__.py`):

```python
from joker_sdk import (Alarm, App, EventOutput, Input, Instant, Internal, Output, Param, Sample,
                       UtcSeconds, app, cycle, record, state, task)

@record
class Plan:
    target_soc: float
    limit_kw: float
    valid_until: UtcSeconds

    @staticmethod
    def from_peak(peak_kw: float, limit_kw: float, now: Instant) -> "Plan":
        excess = max(peak_kw - limit_kw, 0.0)
        target = min(max(0.3 + excess / 500.0, 0.3), 0.95)
        return Plan(target_soc=target, limit_kw=limit_kw, valid_until=now.utc_seconds() + 30 * 60)

@record
class LowSoc:
    soc: float

@state
class Ctl:
    integral: float = 0.0

@app(name="peak_shaver", version="1.4.2")
class PeakShaver(App):
    grid_power = Input(float, unit="kW", quantity="active_power")
    soc = Input(float, unit="1")
    battery = Output(float, unit="kW", quantity="active_power")
    limit = Param(float, unit="kW", range=(0, 5000), default=400.0)
    reserve = Param(float, unit="%", range=(0, 50), default=10.0)
    plan = Internal(Plan)
    plan_stale = Alarm(severity="medium", message="plano expirado")
    low_soc = EventOutput(LowSoc)

    @cycle("500ms", wcet="100ms", state=Ctl)
    def control(self, st: Ctl) -> None:
        grid = self.grid_power.get()            # QualityError → o tick desta tarefa termina aqui
        soc = self.soc.get()
        plan = self.plan.get_or_default()
        stale = plan.valid_until < self.now().utc_seconds()
        self.plan_stale.set(stale)
        limit = self.limit.get() if stale else min(plan.limit_kw, self.limit.get())
        excess = grid - limit
        st.integral = min(max(st.integral + excess * self.dt().seconds, -50.0), 50.0)
        floor = self.reserve.get() / 100.0      # % → fração, como o soc
        self.battery.set(0.0 if soc <= floor else max(excess + 0.1 * st.integral, 0.0))

    @task.every("15m", jitter="30s")
    async def replan(self) -> None:
        now = self.now()
        hist = await self.grid_power.history(now - "24h", now, step="15m")
        peak = max(hist.values(), default=0.0)
        self.plan.set(Plan.from_peak(peak, self.limit.get(), now))

    @task.on("soc")
    async def watch_soc(self, s: Sample[float]) -> None:
        if s.is_good() and s.value < 0.05:
            self.low_soc.emit(LowSoc(soc=s.value))
```

**A diferença deliberada:** o `control` em Python pede `"500ms"`, porque 200 ms está abaixo do mínimo de Python (§19). Num site com o grupo `fast` a 200 ms, a versão Python não liga a esse grupo (`J-E0854`), e é o `jk check` que o diz. O controlo integral usa `dt()`, e por isso a lógica não muda com o período (04 §3.2).

**Correspondência:**

| Conceito | Rust | TypeScript | Python |
|---|---|---|---|
| ler com qualidade | `io.soc.get()?` | `io.soc.get()` (lança) | `self.soc.get()` (lança) |
| interno com omissão | `io.plan.get_or_default()` | `io.plan.getOrDefault()` | `self.plan.get_or_default()` |
| estado do ciclo | `st: &mut Ctl` | `st` da fábrica `state` | `st: Ctl` |
| período efetivo | `io.dt().as_secs_f64()` | `io.dt().seconds` | `self.dt().seconds` |
| proposta | `io.battery.set(sp)` | `io.battery.set(sp)` | `self.battery.set(sp)` |
| histórico | `.history(now - 24.h(), now, 15.mins()).await?` | `await .history({ start: now.minus("24h"), end: now, step: "15m" })` | `await .history(now - "24h", now, step="15m")` |
| só as boas | `hist.iter().filter_map(\|s\| s.good().copied())` | `hist.values()` | `hist.values()` |
| gatilho por mudança | `#[joker::task(on = "soc")]` | `task.on("soc", …)` | `@task.on("soc")` |
| amostra do gatilho | `s.get()` | `s.isGood()`, `s.value` | `s.is_good()`, `s.value` |

**A mesma app com o modelo, na 1.1** (reservado; na v1 a porta `Model` não se liga, o componente é recusado em `prod` com `J-E0003`, e em `dev` o `infer` levanta `UnavailableError` com `J-E0601`):

```python
    forecast = Model(inputs="f32[1,96,4]", outputs="f32[1,96]")
    # em replan: y = await self.forecast.infer([x]); y.quality, y.outputs[0]
```

```ts
  models: { forecast: model({ inputs: "f32[1,96,4]", outputs: "f32[1,96]" }) },
  // em replan: const y = await io.forecast.infer([x]);
```

### 20.2 `pump_duty`: classes, `Many<Pump>`, métodos, consultas, ações

Uma app de negócio sobre `std/pump@1` (17 §3.1): arranca a bomba principal quando o nível desce, escolhe a principal **no início de cada pedido** (a saudável com menos horas), passa à seguinte quando a principal falha o arranque, e pede o rearme em async. É uma versão reduzida do §9.1 do documento 14, com a correção do achado A6 (§33): lá, a principal é reescolhida em todos os ticks, e duas bombas com horas próximas trocam de principal com a bomba a trabalhar.

**Rust** (ilustrativo):

```rust
use joker::prelude::*;
use joker::classes::std::{Pump, PumpCycle};

#[derive(Record)]
pub struct PumpFailed { pump: BoundedString<32>, n: u64 }

#[joker::app(name = "pump_duty", version = "1.0.0")]
pub struct PumpDuty {
    #[device(requires = ["hour_counter"])] pumps: Many<Pump, 1, 8>,
    #[input(unit = "m")]                   level: Input<f64>,
    #[param(unit = "m", default = 2.5)]    start_level: Param<f64>,
    #[param(unit = "m", default = 3.6)]    stop_level: Param<f64>,
    #[mailbox(to = "async")]               failures: Mailbox<PumpFailed, 8>,
    #[publish]                             running: Published<u64>,
}

#[derive(Default)]
struct Duty { demand: bool, lead: usize, failed: [bool; 8], n: u64 }

#[joker::cycle(period = "500ms")]
fn control(io: &mut CycleIo<PumpDuty>, st: &mut Duty) -> Result<()> {
    let level = io.level.get()?;
    if !st.demand && level < io.start_level.get() {
        st.demand = true;
        let hours = |p: &PumpCycle<'_>| p.hours().unwrap_or(f64::INFINITY);
        if let Some(p) = io.pumps.healthy().min_by(|a, b| hours(a).total_cmp(&hours(b))) {
            st.lead = p.index();
        }
    } else if st.demand && level > io.stop_level.get() {
        st.demand = false;
    }
    let n = io.pumps.len();
    let mut running: u64 = 0;
    for p in io.pumps.iter() {                         // os métodos dos handles são &self
        let i = p.index();
        if st.demand && i == st.lead { p.start() } else { p.stop() }
        let failed = p.failed_to_start()?;
        if failed && !st.failed[i] {
            st.n += 1;
            let pump = BoundedString::try_from(p.name()).unwrap_or_default();
            let _ = io.failures.try_send(PumpFailed { pump, n: st.n });   // cheia: perda contada
            st.lead = (st.lead + 1) % n;
        }
        st.failed[i] = failed;
        if p.running().unwrap_or(false) { running += 1; }
    }
    io.running.set(running);
    Ok(())
}

#[joker::task(every = "5s")]
async fn handle_failures(io: &TaskIo<PumpDuty>) -> Result<()> {
    while let Some(f) = io.failures.try_recv() {
        let Some(p) = io.pumps.get(f.pump.as_str()) else { continue };
        match p.reset().key(&format!("reset:{}:{}", f.pump.as_str(), f.n)).await {
            Ok(()) => log::info!("rearme pedido"; pump = f.pump.as_str()),
            Err(e) => log::warn!("rearme falhou"; pump = f.pump.as_str(), code = e.error.code().as_str()),
        }
    }
    Ok(())
}
```

**TypeScript**:

```ts
import { ActionError, app, cycle, input, log, mailbox, many, output, param, published, record, t, task } from "@joker/sdk";
import { Pump } from "@joker/classes-std";

const PumpFailed = record({ pump: t.string({ maxBytes: 64 }), n: t.u64() });

export default app({
  name: "pump_duty",
  version: "1.0.0",
  devices: { pumps: many(Pump, { min: 1, max: 8, requires: ["hour_counter"] }) },
  inputs: { level: input.f64("m") },
  params: {
    start_level: param.f64({ unit: "m", default: 2.5 }),
    stop_level: param.f64({ unit: "m", default: 3.6 }),
  },
  mailboxes: { failures: mailbox(PumpFailed, { capacity: 8, to: "async" }) },
  published: { running: published(output.u64()) },

  cycles: {
    control: cycle(
      { period: "500ms", state: () => ({ demand: false, lead: 0, failed: new Array<boolean>(8).fill(false), n: 0n }) },
      (io, st) => {
        const level = io.level.get();
        if (!st.demand && level < io.start_level.get()) {
          st.demand = true;
          let best: { index: number; hours: number } | null = null;
          for (const p of io.pumps.healthy()) {
            const h = p.hours.getOr(Infinity);
            if (best === null || h < best.hours) best = { index: p.index, hours: h };
          }
          if (best !== null) st.lead = best.index;
        } else if (st.demand && level > io.stop_level.get()) {
          st.demand = false;
        }
        let running = 0n;
        for (const p of io.pumps) {
          if (st.demand && p.index === st.lead) p.start(); else p.stop();
          const failed = p.failed_to_start();
          if (failed && !st.failed[p.index]) {
            st.n += 1n;
            io.failures.trySend({ pump: p.name, n: st.n });
            st.lead = (st.lead + 1) % io.pumps.length;
          }
          st.failed[p.index] = failed;
          if (p.running.getOr(false)) running += 1n;
        }
        io.running.set(running);
      },
    ),
  },

  tasks: {
    handle_failures: task.every("5s", async (io) => {
      for (const f of io.failures.drain()) {
        try {
          await io.pumps.get(f.pump).reset({ key: `reset:${f.pump}:${f.n}` });
          log.info("rearme pedido", { pump: f.pump });
        } catch (e) {
          if (!(e instanceof ActionError)) throw e;
          log.warn("rearme falhou", { pump: f.pump, code: e.code });
        }
      }
    }),
  },
});
```

**Python**:

```python
import math

from joker_sdk import (ActionError, App, Input, Mailbox, Param, Published, app, cycle, devices,
                       field, log, record, state, task, u64)
from joker_classes.std import Pump

@record
class PumpFailed:
    pump: str
    n: u64

@state
class Duty:
    demand: bool = False
    lead: int = 0
    failed: list[bool] = field(default_factory=lambda: [False] * 8)
    n: int = 0

@app(name="pump_duty", version="1.0.0")
class PumpDuty(App):
    pumps = devices(Pump, 1, 8, requires=["hour_counter"])
    level = Input(float, unit="m")
    start_level = Param(float, unit="m", default=2.5)
    stop_level = Param(float, unit="m", default=3.6)
    failures = Mailbox(PumpFailed, capacity=8, to="async")
    running = Published(u64)

    @cycle("500ms", state=Duty)
    def control(self, st: Duty) -> None:
        level = self.level.get()
        if not st.demand and level < self.start_level.get():
            st.demand = True
            lead = min(self.pumps.healthy(), key=lambda p: p.hours.get_or(math.inf), default=None)
            if lead is not None:
                st.lead = lead.index
        elif st.demand and level > self.stop_level.get():
            st.demand = False
        running = 0
        for p in self.pumps:
            if st.demand and p.index == st.lead:
                p.start()
            else:
                p.stop()
            failed = p.failed_to_start()
            if failed and not st.failed[p.index]:
                st.n += 1
                self.failures.try_send(PumpFailed(pump=p.name, n=st.n))
                st.lead = (st.lead + 1) % len(self.pumps)
            st.failed[p.index] = failed
            if p.running.get_or(False):
                running += 1
        self.running.set(running)

    @task.every("5s")
    async def handle_failures(self) -> None:
        for f in self.failures.drain():
            try:
                await self.pumps.get(f.pump).reset(key=f"reset:{f.pump}:{f.n}")
                log.info("rearme pedido", pump=f.pump)
            except ActionError as e:
                log.warn("rearme falhou", pump=f.pump, code=e.code)
```

- Um `QualityError` em `p.failed_to_start()` termina o tick **depois** das propostas das bombas anteriores, que ficam; as seguintes mantêm o lease do tick anterior até expirar (16 A2). É a semântica do `?` do Rust.
- `p.start()` no ciclo é um registo de método (17 §4.1); o `jk check` sabe que escreve `run_cmd`. `p.reset()` só existe em async, com chave de idempotência derivada do número da falha: uma nova tentativa da mesma falha não repete o rearme (16 E9).
- A política de rearme é do site: o `permit` e o `role` da ação vêm do manifesto (17 §4.2), e um `denied` chega como `ActionError`.

---

# Parte B — Bibliotecas cliente da API HTTP local

## 21. Pacotes e geração

| Biblioteca | Registo | Import | Dependências | Quem a usa |
|---|---|---|---|---|
| `joker-client` (Rust) | crates.io | `joker_client` | `tokio`, `hyper` 1 + `hyper-util`, `rustls`, `tokio-tungstenite`, `serde` | a `jk`, o Deck (via túnel, 1.3), ferramentas em Rust |
| `joker-client` (Python) | PyPI | `joker_client` | `httpx` ≥ 0.27, `websockets` ≥ 13, `pydantic` ≥ 2; extra `[pandas]`: `pandas` ≥ 2.2, `pyarrow` ≥ 15 | notebooks, scripts de dados, integração |
| `@joker/client` | npm | `@joker/client` | `undici` ≥ 6, `ws` ≥ 8; Node ≥ 20 | scripts de comissionamento, integrações, o Studio (1.3) |

**Tipos gerados, API escrita à mão.** As três bibliotecas têm duas camadas:

1. **Camada gerada** a partir de `spec/openapi/joker-api.yaml` (ref/09), com o comando `cargo xtask gen-clients`:
   - os **esquemas** (Rust com `typify`; Python com `datamodel-code-generator` para modelos pydantic v2 com `extra = "ignore"`; TS com `openapi-typescript`);
   - uma **tabela de operações** por `operationId`: método, caminho, parâmetros, papel mínimo, se é mutável e se é idempotente.
   Os ficheiros gerados ficam no repositório, e o CI corre `cargo xtask gen-clients --check`: uma diferença entre o OpenAPI e os clientes falha o build.
2. **Camada escrita à mão** por cima: ligação (socket Unix, mTLS), contextos, anti-replay (`nonce`, `ts`), novas tentativas, erros `problem+json` como exceções com código, paginação, subscrições WebSocket, conversões de unidades e tempo, e a exportação para pandas.

Porque não tudo gerado: os geradores de OpenAPI não tratam sockets Unix, mTLS com contextos, WebSocket, anti-replay nem *streaming*, e o resultado não é idiomático em nenhuma das três linguagens. Porque não tudo à mão: os esquemas derivariam do ref/09 sem ninguém dar por isso. A tabela de operações gerada é o ponto de encontro: um método da camada escrita à mão só chama um `operationId` que exista.

**Versões.** A major do cliente é a da API (`/v1` → clientes 1.x). Um cliente 1.y fala com qualquer nó 1.x: campos desconhecidos são ignorados e valores de enum desconhecidos chegam como `unknown` (ref/00 §1, regra 2). No `connect`, o cliente lê `getInfo` (versão da API e a lista `features`, ref/09 §5.1) e, se precisar, `getVersion`; chamar uma operação que o nó não tem (pela `x-joker-since` do OpenAPI ou por uma `feature` em falta) dá `J-E0883` com a versão do nó, antes de enviar o pedido.

---

## 22. Ligação e nós

### 22.1 Endereços

Os endereços são os do `--node` (ref/02 §3.3.1, e `nodes.<nome>.address` do §2.3.2): `local`, um nome de contexto, `unix:<caminho>`, `<host>:<porta>` e `ssh://[<utilizador>@]<host>[:<porta>]`, e só estes (ref/09 §2.1, documento 07 §1):

| Forma | Transporte | Autenticação | Papel |
|---|---|---|---|
| `unix:/run/joker/api.sock` | HTTP/1.1 sobre socket Unix | credenciais do processo (`SO_PEERCRED`) | `local:uid=…`, `local:group=…` (ref/02 §3.3.2, 03 §13) |
| `<host>:<porta>` (porta 7443 por omissão) | HTTP/2 sobre TLS 1.3 | **mTLS** sempre (01 §14); o certificado do nó verifica pela CA **e** tem o nome do nó no SAN (`J0912`) | pelo CN do certificado (`cert:CN=ops-*`) |
| `ssh://[<utilizador>@]<host>[:<porta>]` | túnel SSH até ao socket Unix do nó | a do SSH | a do utilizador SSH no nó (ref/02 §3.3.2) |
| `nodes.<n>.via = "deck:<nome>"` | sessão remota pelo Deck | a do Deck | **1.3**, reservado (ref/02) |

- O Rust implementa `ssh://` com a mesma crate que a `jk`. Python e TS usam o `ssh` do sistema com reencaminhamento de socket (`ssh -N -L <socket temporário>:/run/joker/api.sock`), que exige OpenSSH ≥ 6.7; sem `ssh` no `PATH`, `J-E0881` com a indicação.
- Não há modo inseguro nem confiança no primeiro uso (ref/02 §3.3.1).

### 22.2 Resolução

As bibliotecas resolvem o nó **pelas regras da `jk`** (ref/02 §3.3.1 e §2.3.3), para que um script e a `jk` falem com o mesmo nó sem configuração repetida:

1. um endereço explícito (`Client.connect("lisboa-01.example.com:7443", identity=…)`), sem ler configuração;
2. senão, o valor de `node` dado a `Client.from_config(node)`, ou `JK_NODE`, ou `defaults.node` (com a precedência do ref/02: argumento > variável > `.joker/config.toml` do projeto > `config.toml` do utilizador), resolvido pelos 5 passos do ref/02 §3.3.1 (`local`, endereço literal, nome em `[nodes]`, host de um só contexto, `<valor>:7443`);
3. sem nenhum, `local` (o `jk dev` do projeto, ou `/run/joker/api.sock`).

A identidade é a de `[user]` ou a do contexto (`cert`, `key`, `ca`, incluindo `pkcs11:`), ou `JK_CERT`/`JK_KEY`/`JK_CA`. As bibliotecas **leem** a configuração (`JK_CONFIG` incluído) e nunca a escrevem. Os erros são os do ref/02: `J0910` (configuração inválida), `J0911` (nó desconhecido ou ambíguo), `J0912` (certificado do nó), `J0913` (chave com permissões inseguras). A biblioteca expõe a origem da escolha (`client.origin`, por exemplo `"lisboa-01 · de JK_NODE"`), para que um script possa mostrar a mesma linha que a `jk` mostra antes de mudar o nó (ref/02 §3.3.1).

### 22.3 Construção

```python
from joker_client import AsyncClient, Client, Identity

jk = Client.from_config()                                    # regras do §22.2
jk = Client.from_config("lisboa-01", timeout="30s")
jk = Client.connect("unix:/run/joker/api.sock")
jk = Client.connect("lisboa-01.example.com:7443",
                    identity=Identity(cert="ops.crt", key="ops.key", ca="site-ca.crt"))
async with AsyncClient.from_config("lisboa-01") as ajk:     # a mesma API, com await
    ...
```

```ts
import { Client } from "@joker/client";
const jk = await Client.fromConfig();                        // regras do §22.2
const jk2 = await Client.connect("lisboa-01.example.com:7443", {
  identity: { cert: "ops.crt", key: "ops.key", ca: "site-ca.crt" },
});
```

```rust
let jk = joker_client::Client::from_config(None).await?;
let jk = joker_client::Client::connect(&"unix:/run/joker/api.sock".parse()?, None).await?;
```

Python tem cliente **síncrono** (`Client`, para notebooks e scripts) e **assíncrono** (`AsyncClient`), com os mesmos métodos. TS e Rust são só assíncronos.

---

## 23. Superfície

Cada método corresponde a um `operationId` do OpenAPI (`spec/openapi/joker-api.yaml`), que é o da ref/09. A coluna `jk` diz que comando da CLI usa a operação, porque a `jk` é só um cliente (01 §14, 20 §2).

| Operação | `jk` | `operationId` | Python | TypeScript | Papel | Versão |
|---|---|---|---|---|---|---|
| informação do nó | `jk status` | `getInfo`, `getVersion` | `info()`, `version()` | `info()`, `version()` | viewer | 1.0 |
| estado | `jk status [--instances]` | `getStatus` | `status()` | `status()` | viewer | 1.0 |
| ler sinais | `jk read <sel>` | `readValues` (`queryValues` com muitos seletores) | `read(sel, *, coherent=False, mono=False)` | `read(sel, o?)` | viewer | 1.0 |
| forçar leitura | `jk read --refresh` | `refreshValues` | `read(sel, refresh=True)` | `read(sel, { refresh: true })` | operator | 1.0 |
| subscrever | `jk read --watch`, `jk logs --follow` | `websocket` (e `sse`) | `subscribe(...)` | `subscribe(...)` | viewer | 1.0 |
| histórico | `jk history` | `queryHistory` (`getHistory`) | `history(sel, start, end, *, step=None, agg=None)` | `history(sel, o)` | viewer | 1.0 |
| exportar Parquet | `jk export --parquet` | `queryHistory` em NDJSON | `export(sel, start, end, path)` (o Parquet é escrito pela biblioteca) | `export(sel, o)` | viewer | 1.0 |
| pré-visualizar escrita | `jk write` (antes de confirmar) | `previewManualWrite` | `write(…, dry_run=True)` | `write(…, { dryRun: true })` | viewer | 1.0 |
| escrita manual | `jk write` | `manualWrite` | `write(path, value, *, ttl, reason, level=None, takeover=False)` | `write(path, value, o)` | operator | 1.0 |
| libertar nível | `jk release` | `releaseManual` | `release(path, *, level=None, reason)` | `release(path, o)` | operator | 1.0 |
| pré-visualizar método | `jk call` (antes de confirmar) | `previewMethodCall` | `call(…, dry_run=True)` | `call(…, { dryRun: true })` | viewer | 1.0 |
| chamar método | `jk call` | `callMethod` | `call(target, args=None, *, ttl, reason)` | `call(target, args, o)` | operator | 1.0 |
| invocar ação | `jk action` | `invokeAction`, `getInvocation`, `cancelInvocation` | `invoke(target, args=None, *, key=None, timeout=None)` | `invoke(target, args, o?)` | o do manifesto (mín. operator) | 1.0 |
| alarmes | `jk alarms`, `jk alarm ack/shelve` | `listAlarms`, `getAlarm`, `ackAlarm`, `shelveAlarm`, `unshelveAlarm`, `listAlarmEvents` | `alarms.list()`, `.ack(id, *, reason=None)`, `.shelve(id, ttl, reason)`, `.unshelve(id)`, `.events(…)` | idem, camelCase | viewer / operator | 1.0 |
| trips | `jk trips`, `jk reset` | `listTrips`, `resetTrip`, `resetTrips` | `trips.list()`, `trips.reset(path, *, reason)` | idem | viewer / operator | 1.0 |
| decisões | `jk decisions` | `listDecisions` | `decisions.query(*, output=None, cause=None, start=None, end=None, since=None)` | `decisions.query(o)` | viewer | 1.0 |
| gerações | `jk generations`, `jk apply`, `jk confirm`, `jk rollback` | `listGenerations`, `getGeneration`, `createGeneration`, `activateGeneration`, `confirmGeneration`, `rollbackGeneration`, `discardGeneration` | `generations.list()`, `.current()`, `.get(gen)`, `.apply(bundle, *, confirm=None, dry_run=False)`, `.confirm(gen, *, reason)`, `.rollback(to=None, *, reason)` | idem | viewer / admin | 1.0 |
| apps | `jk app start/stop/restart/logs` | `listApps`, `getApp`, `startApp`, `stopApp`, `restartApp` | `apps.list()`, `.get(name)`, `.start(name)`, `.stop(name, *, reason)`, `.restart(name)` | idem | viewer / operator | 1.0 |
| instâncias | `jk status --instances`, `jk explain <i>` | `listInstances`, `getInstance` | `instances.list()`, `.get(name)` | idem | viewer | 1.0 |
| logs | `jk logs` | `listLogs` | `logs.query(*, app=None, since=None, level=None)` | `logs.query(o)` | viewer | 1.0 |
| intenções | `jk intents`, `jk intent approve/reject` | `listIntents`, `getIntent`, `approveIntent`, `rejectIntent` | `intents.list()`, `.get(id)`, `.approve(id, *, reason)`, `.reject(id, *, reason)` | idem | operator | 1.0 (vazias até à 1.1) |
| qualquer outra | — | qualquer | `request(operation_id, *, params=None, body=None)` | `request(opId, o)` | a da operação | 1.0 |

`apply` compõe `findMissingBlobs`, `startBlobUpload`/`uploadBlobChunk`/`finishBlobUpload`, `createGeneration` e `activateGeneration`; `rollback` pede primeiro o `ETag` (`getGeneration` de `current`) para o `If-Match`.

Regras que a camada escrita à mão impõe **antes** de enviar:

- **`ttl` e `reason` são obrigatórios** em `write` e `call` (16 A10, D19): em Python, argumentos *keyword-only* sem omissão (um `TypeError` se faltarem); em TS, propriedades obrigatórias do tipo; em Rust, o *builder* não constrói sem eles. Um `ttl` acima do `max_manual` é recusado pelo nó (`J-E1102`).
- **`dry_run`** chama a operação de pré-visualização (`previewManualWrite`, `previewMethodCall`) e devolve o que a `jk write` mostra antes de pedir confirmação: o envelope, o nível ativo e o resultado previsto (07 §4). A escrita que se segue leva `If-Match` com a geração que a pré-visualização viu. A confirmação interativa é da `jk`, e não da biblioteca.
- `level` omitido: `manual:4` (D19, ref/09 §5.6). Um nível ocupado por outro principal é `J-E1105`; `takeover=True` substitui-o (A13).
- `apply` envia o *bundle* da geração (manifesto, lock, contratos, artefactos) pelo upload em blocos da ref/09 §5.13 (ref/00 §10) e devolve a geração pendente.
- `history` e `export` pedem `application/x-ndjson` e paginam pelo cabeçalho `Link` (ref/09 §3.5). O nó não produz Arrow nem Parquet (ref/09 §12): a biblioteca converte o NDJSON para `pandas`/Arrow/Parquet do lado do cliente.

---

## 24. Tipos, valores e erros

### 24.1 Amostras e tempo

- Uma amostra é o objeto JSON do ref/00, §4.2, com os mesmos campos (`path`, `value`, `unit`, `quality`, `reason`, `ts_src`, `ts_acq`, `seq`, e `mono_ns` com `mono=True`). Em Python, `Sample` (pydantic); em TS, `interface Sample`. Nos nomes de campos, a API das bibliotecas segue o estilo da linguagem (`tsAcq` em TS), e os valores (`"comm_failure"`) são os do JSON.
- `Sample.get()` levanta `QualityError` (`J-E0800`) se a qualidade não for `good`; `get_or(d)`/`getOr(d)`; `value` é o valor tal como veio. A mesma regra das apps: **a biblioteca nunca devolve só um número de uma amostra `bad`**.
- Os instantes UTC são `UtcTime` (`.ns`, `.isoformat()`, `.datetime()` em Python; `.ns: bigint`, `.toISOString()`, `.toDate()` em TS), porque o `datetime` do Python e o `Date` do JS perdem os nanossegundos do ref/00, §3.
- `i64` e `u64` em TS são `bigint`, também quando o JSON os traz como número (ref/00 §4.1). Em Python, `int`.
- `bits` trazem `bits` com os nomes ativos (ref/00 §4.1); `enum` trazem o nome da variante.

### 24.2 Valores na entrada

`write("pcs.p_set", 50)` escreve 50 na unidade do sinal; `write("pcs.p_set", "50 kW")` envia o literal de quantidade, e o nó converte (ref/00 §4.1; dimensão errada é `J0107`, devolvido como erro da API). As durações aceitam literais (`"10m"`) e, em Python, `timedelta`; em Rust, `std::time::Duration`. Os instantes aceitam RFC 3339, `UtcTime`, `datetime` com fuso (Python) ou `Date` (TS); um `datetime` sem fuso é `TypeError`, porque não há hora local implícita.

### 24.3 Erros

```python
class ApiError(JokerClientError):          # resposta application/problem+json (ref/00 §8)
    code: str                              # "J-E1203", "J0107", …
    title: str
    detail: str
    status: int
    spans: list[Span] | None
    exit_code: int                         # o código de saída da jk (07 §8) para este erro

class JokerClientError(Exception):         # erros do lado do cliente (J-E088x)
    code: str
```

```ts
export declare class JokerClientError extends Error { readonly code: string; readonly exitCode: number }
export declare class ApiError extends JokerClientError {
  readonly title: string; readonly detail: string; readonly status: number; readonly spans?: Span[];
}
```

Uma resposta que não cumpre o esquema gerado (campo obrigatório em falta, tipo errado) é `J-E0884`, com o `operationId` e o caminho do campo; campos a mais nunca são erro. `exit_code`/`exitCode` segue o documento 07, §8: 2 para erros de validação (`J01xx`–`J06xx`), 3 para recusas (403, envelope, assinatura), 4 para timeout ou nó inacessível (`J-E0881`), 5 para confirmação expirada, 1 para o resto. É o que a `jk` usa para sair, e o que um script de CI pode usar também.

### 24.4 Anti-replay, novas tentativas e idempotência

- Cada pedido mutável leva os cabeçalhos `Joker-Nonce` (ULID) e `Joker-Timestamp` (UTC, RFC 3339) (ref/00 §9, ref/09 §2.5), gerados pela biblioteca, também no socket Unix.
- **Leituras** (`GET` e subscrições): até 3 novas tentativas com *backoff* exponencial (0,2 s → 1,6 s) em erro de ligação e em 503.
- **Escritas, métodos, alarmes, gerações:** só se repete um pedido que **não chegou ao nó** (erro de ligação antes de enviar) ou um 503 com `Retry-After`. Cada tentativa leva um `nonce` novo, porque um `nonce` repetido é recusado como replay. Repetir uma escrita manual é seguro: o mesmo escritor, nível e valor só renovam o TTL.
- **Ações:** a chave de idempotência mantém-se entre tentativas (gerada pela biblioteca se não for dada, e devolvida no resultado), e o nó devolve o resultado anterior dentro do `dedup_window` (16 E9).
- Nunca se repete um 4xx.
- **Relógio:** com o relógio do nó `synced`, um `Joker-Timestamp` fora de ±30 s é recusado (`J-E1021`); com `holdover` ou `unsynced`, o nó verifica só o nonce e responde com `Joker-Clock` (D16, ref/09 §2.5). No `connect`, a biblioteca compara o seu relógio com o cabeçalho `Date` da resposta e, acima de 10 s de diferença, emite o aviso `J-E0885` no log; um `J-E1021` leva a mesma dica.
- Tempos limite por omissão: ligação 5 s, pedido 30 s, histórico e exportação 120 s; todos configuráveis.

---

## 25. Subscrições

```python
for msg in jk.subscribe(signals=["meter.**", "pcs.p_set"], alarms=True, generations=True):
    match msg:
        case SampleMsg(sample=s): ...
        case AlarmMsg(alarm=a): ...
        case GenerationMsg(generation=g): ...
        case Gap(stream=st, lost=n): ...           # houve perda; o que se perdeu está no historian
```

```ts
for await (const msg of jk.subscribe({ signals: ["meter.**", "pcs.p_set"], alarms: true })) {
  if (msg.type === "sample") { /* msg.sample */ } else if (msg.type === "gap") { /* msg.lost */ }
}
```

- Uma só ligação WebSocket (`websocket`, subprotocolo `joker.v1`, ref/09 §6) com uma subscrição por tópico: `signals`, `alarms`, `events`, `generations`, `intents`, `logs`, e os restantes do §6.4 da ref/09. As mensagens `data`, `resync` e `gap` do servidor chegam como `SampleMsg`, `AlarmMsg`, …, `Resync` e `Gap`.
- **Reconexão automática** com o `resume` da ref/09 §6.7: a biblioteca guarda a `session` do `welcome` e o último `seq` de cada subscrição, e retoma dentro da `resume_window` (60 s). Se o nó já não tiver o que falta (`resume_unavailable`, `J-E1907`, ou `boot_id` novo), a biblioteca entrega um `Gap` (e não uma exceção) com o número perdido e subscreve de novo, e regista `J-E0886`. A telemetria nunca bloqueia o nó (ADR 0013): um cliente lento perde mensagens, contadas.
- *Buffer* local de 10 000 mensagens; acima, a biblioteca descarta as mais antigas e entrega um `Gap`.
- O cliente síncrono do Python devolve um iterador bloqueante (a ligação corre numa thread da biblioteca).

---

## 26. Exemplos

### 26.1 Notebook: do historian para pandas

```python
from joker_client import Client

jk = Client.from_config("lisboa-01")
info = jk.info()
print(info.node, info.generation, info.version)

h = jk.history(["meter.p_total", "pcs.p_ac", "bms.soc"],
               start="2026-09-23T00:00:00Z", end="2026-09-24T00:00:00Z", step="1m")
df = h.to_pandas()                          # índice DatetimeIndex UTC (ns); colunas (caminho, campo)
power = df.xs("value", axis=1, level="field")
ok = df.xs("quality", axis=1, level="field") == "good"
power.where(ok).resample("15min").mean().plot()        # os valores não good ficam NaN, à vista

decisions = jk.decisions.query(output="pcs.p_set", start="2026-09-23T17:00:00Z",
                               end="2026-09-23T19:00:00Z").to_pandas()
decisions["outcome"].value_counts()
```

- `to_pandas()` exige o extra `joker-client[pandas]` (`J-E0888` sem ele). Os campos por caminho são `value`, `quality`, `reason` e `unit` (este como atributo, em `df.attrs["units"]`). `layout="long"` dá uma linha por amostra (`ts`, `path`, `value`, `quality`, `reason`).
- **A qualidade vem sempre com o valor.** Não há opção para a deitar fora; quem quer só os bons filtra, como acima.
- A biblioteca pede o histórico em NDJSON (`Accept: application/x-ndjson`, ref/09 §3.5) e constrói o `DataFrame` em blocos; o nó não produz Arrow (ref/09 §12).
- Guarda de tamanho: acima de 5 000 000 de pontos, `history()` falha com `J-E0889` antes de pedir, com o número estimado e a sugestão de `step` ou `export()` para Parquet. `max_points=` muda o limite.

### 26.2 Script de comissionamento em TypeScript

Testa a limitação de cada inversor em degraus, com escritas manuais com TTL, e confirma a geração pendente só se tudo tiver respondido (exemplo 01, §7).

```ts
// commission.ts — node --experimental-strip-types commission.ts
import { setTimeout as sleep } from "node:timers/promises";
import { ApiError, Client } from "@joker/client";

const jk = await Client.fromConfig("lisboa-01");
const gen = await jk.generations.current();
if (gen.state !== "confirming") {
  throw new Error(`a geração ${gen.gen} está em ${gen.state}; nada a confirmar`);
}

type Row = { inv: string; pct: number; outcome: string; p_kw: number | null };
const rows: Row[] = [];
try {
  for (const inv of ["inv1", "inv2", "inv3"]) {
    for (const pct of [100, 60, 30, 0]) {
      const w = await jk.write(`${inv}.w_max_lim_pct`, `${pct} %`, {
        ttl: "5m", level: "manual:4", reason: "comissionamento: degraus de limitação",
      });
      await sleep(10_000);
      const [p] = await jk.read(`${inv}.ac.p`);
      rows.push({ inv, pct, outcome: w.outcome.outcome, p_kw: p.isGood() ? Number(p.value) : null });
    }
    await jk.release(`${inv}.w_max_lim_pct`, { level: "manual:4" });
  }
} catch (e) {
  if (e instanceof ApiError) console.error(`${e.code}: ${e.detail}`);
  throw e;                                   // os TTL de 5 min devolvem as saídas às apps sozinhos
}

console.table(rows);
if (rows.every((r) => r.outcome === "accepted" && r.p_kw !== null)) {
  await jk.generations.confirm(gen.gen, { reason: "comissionamento: limitação dos inversores OK" });
} else {
  console.error("não confirmado: a janela de confirmação expira e o nó faz rollback (ADR 0015)");
  process.exitCode = 5;
}
```

O script não precisa de limpar nada se falhar a meio: cada escrita manual tem TTL (A10) e expira sozinha, e a geração volta atrás se a confirmação não chegar.

### 26.3 Rust, na `jk`

```rust
use joker_client::{Client, ManualWrite};
use std::time::Duration;

let jk = Client::from_config(node.as_deref()).await?;
let preview = jk
    .write(ManualWrite::new("pcs.p_set", "50 kW")
        .ttl(Duration::from_secs(30 * 60))
        .reason(&args.reason)
        .dry_run(true))
    .await?;
render_envelope(&preview);                   // o que a jk mostra antes de pedir confirmação
if confirm()? {
    let report = jk.write(preview.into_request().dry_run(false)).await?;
    println!("{}", report.outcome);
}
```

---

# Parte C — Comportamento, estresse, decisões

## 27. Comportamento esperado

| Tema | Regra | Fonte |
|---|---|---|
| qualidade | os acessores seguem a tabela de 16 §3.1 nos SDKs e nas bibliotecas cliente; nenhuma API devolve só o valor de uma amostra não `good` sem ser `get_or`, `get_uncertain` ou `sample` | 16 §3.1, invariante 4 |
| leitura no ciclo | do latch; estável dentro do tick | 02 §6.1 |
| leitura em async | do slot; `snapshot` para coerência | 02 §6.4 |
| escrita no ciclo | proposta; a última do tick vence; `last_outcome` no tick seguinte | 16 A4, 02 §7.1 |
| escrita em async | proposta para o grupo dono; `await` devolve o resultado final | 01 §5 |
| ciclo e async da mesma app | o mesmo escritor no árbitro; a proposta de ciclo vence a async no mesmo tick | 16 A1, A4 |
| erros de tarefa | `JokerError` = `Err` (propostas ficam); outra exceção = `panic` (`J-E0816`: propostas do tick descartadas, conta para o crashloop, sem recriar a instância); trap Wasm = trap | ref/04 §2.6.1, 01 §9; §9.3 |
| métodos de classe | registo expandido pelo núcleo pela ordem das chamadas; `pending()` = `None` | 17 §4.1 |
| ações | só async; chave de idempotência; `dedup_window` | 02 §8, 16 E9 |
| determinismo | no ciclo, relógios = latch, PRNG semeado pelo host, NaN canónicos, sem I/O; o mesmo traço dá as mesmas propostas | ADR 0007, 09 ataque 10 |
| hot-swap | a instância nova é criada ao lado; o estado passa por `export-state`/`migrate` | 01 §9 |
| escritas manuais pelos clientes | TTL e motivo obrigatórios; `nonce` e `ts` sempre | 16 A10, ref/00 §9 |
| compatibilidade | leitores tolerantes nas bibliotecas cliente; ficheiros de pessoas estritos (`[tool.joker]`, `"joker"`) | ref/00 §1 |

---

## 28. Estresse

**E1 — Estado em `self` ou numa global, em Python.** Uma pessoa escreve `self.last_plan = plan` numa tarefa async e lê-o no ciclo. *O que acontece:* `J-E0803` na primeira atribuição, com a explicação das duas instâncias e a indicação de `Internal`. Com `global last_plan`, não há erro em runtime (o ciclo vê sempre o valor inicial); o `jk build` avisa (`J-E0858`). *Quebrou* na primeira versão, em que `self` era um objeto normal: a app "funcionava" nos testes nativos e falhava em produção. **Corrigido:** `self` só de leitura, e o harness corre sempre o `.wasm` real, com as duas instâncias.

**E2 — Nomes em `camelCase` em TS.** O exemplo do documento 04, §4.1, declara `zoneTemp`. *O que acontece:* `J-E0857` no `jk build`, com a sugestão `zone_temp`. O TS convida ao `camelCase`, mas os nomes do utilizador aparecem no manifesto, na CLI, no historian e nas outras linguagens (ref/00 §2). **Corrigido** no exemplo (achado A1).

**E3 — Pausa de GC numa tarefa TS a 100 ms.** Uma tarefa que aloca muito tem uma pausa de 15 ms de vez em quando. *O que acontece:* o `wcet` por omissão é 20 ms e o deadline por epoch é 40 ms, e a pausa cabe. O `jk bench` mede o **máximo** (e não o p99,9) e grava-o no lock, e a admissão usa-o. Uma pausa acima do deadline interrompe a tarefa, as propostas do tick perdem-se e os leases seguem (01 §9). *Quebrou* quando a admissão usava o p99,9 como para Rust: uma cauda de GC fora do p99,9 dava deadlines falhados em campo. **Corrigido:** máximo em tier 2 (§19).

**E4 — `try: … except Exception: pass` à volta do controlo.** A exceção de qualidade é engolida e a tarefa continua com o valor da iteração anterior de uma variável local. *O que acontece:* é código válido, e o SDK não o pode impedir. O `jk build` avisa (`J-E0863`) quando uma tarefa de ciclo tem uma captura genérica à volta de chamadas a `get()`, e a documentação do SDK abre com o caminho certo (`get_or` ou deixar a exceção sair).

**E5 — Uma exceção qualquer a meio do tick, depois de dois `set()`.** Um `ZeroDivisionError`. *O que acontece:* as duas propostas são descartadas, como num `panic` do Rust (01 §9), e o erro conta para o crashloop. A instância **não** é recriada (§9.3). *Quebrou* na primeira versão, que tratava todas as exceções como `Err` e deixava passar propostas calculadas até meio. **Corrigido:** só os `JokerError` mantêm as propostas.

**E6 — Números acima de 2⁵³ em TS.** Um contador `u64` de energia em Wh e os instantes em ns. *O que acontece:* chegam como `bigint`, nas apps e na biblioteca cliente, também quando o JSON os traz como número. Um `number` numa porta `i64` é um erro de tipos; em JS sem tipos, `LimitError` (`J-E0806`) se não for um inteiro seguro. Nunca há arredondamento silencioso.

**E7 — Um membro de classe chamado `name`, ou `from`.** *O que acontece:* `name` é recusado pelo `jkc`, porque os nomes da API dos handles são reservados (ref/00 §2; achado A8, resolvido). `from` passa: o gerador Python acrescenta `_` (`p.from_()`) e o `jkc` avisa; o nome do membro no manifesto e no `.jkc` não muda.

**E8 — API async no ciclo, em Python.** `self.grid_power.history(...)` dentro de um `def` de ciclo. *O que acontece:* `J-E0859` no `jk build` (análise estática das chamadas conhecidas: `history`, `write`, `invoke`, `kv.*`, `secrets.*`, `snapshot`, `subscribe`, métodos de ação), e `J-E0901` em runtime se escapar ao lint (por exemplo, por `getattr`). Um `await` num `def` nem compila. Em TS, é erro de tipos.

**E9 — Efeitos laterais na importação.** `START = time.time()` no topo do módulo, ou `if os.environ.get("SITE") == "porto": …` à volta de uma porta. *O que acontece:* o relógio na declaração é `J-E0810`. O `os.environ` é diferente entre a execução nativa e a pré-inicialização, e o registo diverge: `J-E0862`. O `random` semeado na importação ficaria igual em todas as instâncias por causa do *snapshot*: o SDK volta a semeá-lo no `init`. *Quebrou* na primeira versão, sem a comparação de digests: a app tinha no `joker:meta` portas diferentes das que o código usava, e os handles ficavam trocados. **Corrigido:** a comparação no passo 5 do §18.4.

**E10 — Versões cruzadas.** (a) Uma app TS gerada para `joker:runtime@1.0.0` numa runtime 1.3: corre (ADR 0022). (b) Um `@joker/sdk` novo que gera `@1.3.0` enviado para um nó 1.0: recusado **antes** do deploy pelo agente ou pelo `jk apply`, pelo `wit` do `joker:meta` (M21); o `jk build --world joker:runtime@1.0.0` (ou `world` no `[tool.joker]`) gera para o nó antigo, e as funções da 1.3 levantam `UnavailableError`. (c) Um `joker-client` 1.3 contra um nó 1.0: as operações que o nó não tem dão `J-E0883` antes do pedido, e o resto funciona.

**E11 — Rede instável num script de comissionamento.** A ligação cai a meio de um `write` e de um `invoke`. *O que acontece:* a escrita é repetida com um `nonce` novo (o nó recusaria o antigo como replay), e é segura porque só renova o nível; a ação é repetida com a **mesma** chave, e o nó devolve o resultado da primeira. Com o relógio do portátil 2 minutos adiantado, o nó recusa tudo; o erro traz o código da API e a dica `J-E0885`. *Quebrou* no desenho inicial, que reutilizava o `nonce` nas tentativas.

**E12 — Um notebook pede uma semana em bruto de 5 000 sinais.** *O que acontece:* a estimativa passa de 5 × 10⁶ pontos e `history()` falha com `J-E0889` antes de pedir, com a sugestão de `step` ou de `export()` para Parquet (a biblioteca lê o NDJSON paginado e escreve o Parquet em blocos, sem guardar tudo em memória; o nó não produz Parquet nem Arrow, ref/09 §5.5). Um pedido grande dentro do limite chega em páginas de NDJSON. O historian e o nó nunca ficam à espera do cliente (ADR 0013).

---

## 29. Decisões e alternativas rejeitadas

**D1 — TS: objeto de definição.** Dá os tipos de `io` por domínio sem geração de código e funciona em JS puro. *Rejeitado:* decoradores TC39 (não mudam o tipo de `this` por domínio, e o `this` partilhado esconde as duas instâncias); classes com métodos (o mesmo problema).

**D2 — Python: classe com descritores e `self` como vista só de leitura.** Dá tipos no mypy/pyright e é idiomático. *Rejeitado:* um parâmetro `io` como no documento 04 (sem *mapped types* no Python, `io.x` seria `Any`); funções soltas com um registo global (perde o agrupamento e os tipos).

**D3 — Exceções como `?`.** É a forma idiomática nas duas linguagens, e o comportamento na fronteira da tarefa é o do `Err` do Rust. *Rejeitado:* objetos `Result` em Python (ninguém os usaria); em TS existe `tryGet()` como alternativa, sem ser o caminho principal.

**D4 — Uma exceção não apanhada é um `panic`, mas sem trap.** *Rejeitado:* trap real (recriar uma instância Python custa até 300 ms e perde o estado de todas as tarefas); tratá-la como `Err` (propostas calculadas pela metade).

**D5 — Tier 2 no ciclo, com mínimos de período e admissão pelo máximo.** *Rejeitado:* tier 2 só em async (empurra o controlo lento para tarefas sem latch nem admissão); ciclo sem mínimos (as pausas de GC tornam os grupos rápidos inadmissíveis na prática, e o `jk check` já o diria tarde).

**D6 — A imagem empacotada também em tier 2.** *Rejeitado:* uma chamada `cycle-io.read` por porta, como o documento 09, §2.2, previa (ms por tick num motor JS ou Python).

**D7 — `joker_sdk`, `joker_classes`, `joker_client`, `joker_testing` em Python.** *Rejeitado:* `import joker` (o *namespace* existe no PyPI, e a documentação levaria as pessoas a instalar o pacote errado).

**D8 — `bigint` para `i64`, `u64` e instantes em TS.** *Rejeitado:* `number` (perda silenciosa acima de 2⁵³); `number | bigint` conforme a magnitude (um tipo que muda com o valor parte o código aos 104 dias de *uptime*).

**D9 — Plano B para o async do Component Model.** Se o ComponentizeJS ou o componentize-py não tiverem o async do WASI 0.3 estável quando o SDK sair, a instância async de uma app de tier 2 é servida por uma variante do mundo em que as funções `async` são **bloqueantes**, numa thread dedicada da *pool* `app` (nunca no domínio de ciclo). O SDK mantém a mesma API (`await` resolve de imediato), e perde-se só a concorrência entre tarefas da mesma app: um `history()` lento atrasa as outras tarefas async dessa app. A variante do mundo é o `app-blocking` da ref/04 §2.11, ativada por ADR na Fase 3. *Rejeitado:* adiar o SDK TS para além da v1.0; um SDK sem tarefas async.

**D10 — Harness como extensão nativa (PyO3, napi-rs) da crate `joker-test`.** Um só harness, com o `joker-core` e o wasmtime reais, controlado passo a passo. *Rejeitado:* um subprocesso com protocolo próprio (lento no passo a passo, e mais uma interface para especificar); reimplementar o harness em cada linguagem (três semânticas).

**D11 — Clientes com tipos e operações gerados e ergonomia à mão.** *Rejeitado:* tudo gerado (sem socket Unix, mTLS, WebSocket, anti-replay, nem API idiomática); tudo à mão (deriva silenciosa face ao ref/09).

**D12 — Cliente Python na v1.0, apesar de o SDK Python ser da 1.2.** O cliente é HTTP puro, não depende do componentize-py, e é o que as equipas de dados e de comissionamento pedem primeiro (21 §4). *Rejeitado:* esperar pela 1.2.

**D13 — `health()` calculado no SDK, com vetores de teste partilhados.** A regra é uma função pura das qualidades. *Rejeitado, por agora:* uma consulta implícita `health` avaliada pelo núcleo em cada instância, que mudaria o documento 17 e o registo de sinais. Fica como alternativa se aparecer divergência.

**D14 — Portas ordenadas por nome no `joker:meta`.** *Rejeitado:* ordem de declaração (depende de detalhes do interpretador e muda com uma reorganização inócua do código).

**D15 — `version` opcional no código, `name` obrigatório.** *Rejeitado:* duplicar os dois (ficam dessincronizados); só o pacote (o nome da app é `snake_case` e o do pacote não tem de ser).

---

## 30. Marcações de versão

| Coisa | v1.0 | 1.1 | 1.2 | 1.3 |
|---|---|---|---|---|
| SDK TS (`@joker/sdk`, `@joker/test`, `@joker/classes-*`) | **sim** | | | |
| SDK Python (`joker-sdk`, `joker-testing`, `joker-classes-*`) | API fechada aqui | | **sim** | |
| `set_at`, `write_at` (M6) | nos tipos; `J-E0003` em `prod`, `J-E0601` em `dev` | **sim** | | |
| `propose`, intenções (M18) | nos tipos; `J-E0003` em `prod`, `J-E0601` em `dev` | **sim** | | |
| `Model`, `infer`, frames, streams, blocos | nos tipos; `unavailable` (11, "Preparado para ML") | **sim** | | |
| ações longas com progresso (`invoke_long`) | **sim** (ref/04 §2.4) | | | |
| segredos (`Secret`) | **sim** (ref/04 §2.5.2) | | | |
| clientes Rust, Python, TS | **sim** | | | |
| intenções nos clientes | na tabela de operações; `J-E0883` num nó sem elas | **sim** | | |
| Arrow e Parquet produzidos pelo nó | **não** (NDJSON; conversão no cliente, ref/09 §12) | | | |
| cliente TS no browser | | | | **sim** (Studio) |
| `deck://` (túnel do Deck) | reservado | | | **sim** |
| apps de tier 2 no Joker Micro | **não** (`J0720`) | não | não | não (a 1.4 traz Wasm em MCUs M2, só para Rust) |

---

## 31. Códigos novos (`J-E0800`–`J-E0899`)

| Código | Gravidade | Mensagem | Onde |
|---|---|---|---|
| `J-E0800` | error | leitura com qualidade que o acessor não aceita (`QualityError`) | SDKs, clientes |
| `J-E0802` | error | API de ciclo chamada em async (`dt`, `pending`, `last_outcome`, `set`, receção de caixa do lado errado) | SDKs |
| `J-E0803` | error | atribuição à vista da app: o estado é por domínio; usa `st`, `Internal` ou `Mailbox` | SDKs |
| `J-E0804` | error | `Internal` com campo não escalar ou acima de 64 B codificado | build, runtime |
| `J-E0805` | error | limite de codificação: `string`/`bytes` de sinal > 64 B, string de registo > 1 KiB, lista > 64, chave de idempotência inválida | SDKs |
| `J-E0806` | error | valor fora do tipo da porta (gama inteira, inteiro não seguro, literal de duração inválido, contador negativo) | SDKs |
| `J-E0807` | error | instância desconhecida num `Many` (nome ou posição) | SDKs |
| `J-E0808` | error | membro de uma capacidade que a instância não tem | SDKs |
| `J-E0809` | error | API estável de um mundo posterior ao gerado, indisponível nesta runtime (os itens `@unstable` dão `J-E0003`/`J-E0601`, ref/04 §2.9) | SDKs |
| `J-E0810` | error | chamada ao host durante a declaração da app | build |
| `J-E0811` | error | `kv`: chave ou valor fora dos limites | SDKs |
| `J-E0813` | error | tarefa ou handler mal declarado (domínio, assinatura, gatilho, ação sem handler) | build |
| `J-E0815` | error | estado persistente não codificável, acima de 64 KiB, ou incompatível sem `migrate` | runtime |
| `J-E0816` | error | exceção não apanhada numa tarefa (tratada como `panic`) | runtime |
| `J-E0817` | error | segredo não declarado | SDKs |
| `J-E0818` | error | chamada ao host em modo nativo (fora do Wasm) | SDKs |
| `J-E0850` | error | ponto de entrada inválido: sem app, mais do que uma, sem herdar de `App`, várias linguagens na mesma pasta | build |
| `J-E0851` | error | declaração de parâmetro inválida (`range`, `default`) | build |
| `J-E0852` | error | versão no código diferente da do pacote | build |
| `J-E0853` | error | empacotamento impossível: dependência nativa sem roda `wasm32-wasi`, import `node:*` | build |
| `J-E0854` | error-in-prod | tarefa de ciclo de tier 2 abaixo do período mínimo ou com `wcet` acima de 50% do período | build, `jk check` |
| `J-E0855` | error-in-prod | tier 2 numa arquitetura sem Cranelift (armv7/Pulley) | `jk check`, `jk build --target` |
| `J-E0856` | error | mundo pedido que o SDK não sabe gerar | build |
| `J-E0857` | error | nome inválido, reservado ou repetido na declaração | build |
| `J-E0858` | warning | tarefa escreve numa variável global do módulo (não é partilhada entre ciclo e async) | build |
| `J-E0859` | error | API async chamada numa tarefa de ciclo (análise estática) | build |
| `J-E0860` | error | componente ou `include` acima do tamanho máximo | build |
| `J-E0861` | error | toolchain em falta ou de versão errada (Node, Python, `componentize-py`) | build |
| `J-E0862` | error | a declaração diverge entre a execução nativa e a pré-inicialização | build |
| `J-E0863` | warning | captura genérica de exceções à volta de leituras numa tarefa de ciclo | build |
| `J-E0864` | warning | sem lock do ecossistema: build não reprodutível | build |
| `J-E0881` | error | nó inacessível ou tempo limite (socket, TCP, SSH, permissões do socket) | clientes |
| `J-E0882` | error | o nó recusou a identidade do cliente (certificado de utilizador em falta, expirado ou sem papel) | clientes |
| `J-E0883` | error | operação ou versão da API não suportada pelo nó | clientes |
| `J-E0884` | error | resposta do nó que não cumpre o esquema | clientes |
| `J-E0885` | warning | relógio local desfasado do nó (> 10 s); as escritas podem ser recusadas | clientes |
| `J-E0886` | warning | subscrição com perda (`Gap`) | clientes |
| `J-E0888` | error | dependência opcional em falta (`joker-client[pandas]`) | clientes |
| `J-E0889` | error | resultado estimado acima de `max_points` | clientes |

`J-E0801`, `J-E0812`, `J-E0814`, `J-E0880`, `J-E0887` e o resto da gama ficam livres. Os erros de configuração e de resolução do nó são os do ref/02 (`J0910`–`J0913`), usados tal como estão. O `J-E0901` (API async no ciclo) é do documento 04 e é usado tal como está.

---

## 32. Dependências de outras referências (resolvidas)

O que este documento pedia às outras referências, e como ficou depois da integração (documento 22, §5 e §7).

| # | Referência | Pedido | Como ficou |
|---|---|---|---|
| X1 | ref/04 | chaves `tier`, `lang`, `engine`, `sdk` e os tipos de porta de alarmes, publicados, caixas e segredos no `joker:meta` | **resolvido:** `sdk-info` 0, 3, 4, 5; `port-kind` 4, 5, 9, 10; segredos na chave 13 (§3, §5.4) |
| X2 | ref/04 | handle *i* = posição na lista de portas | **resolvido:** ref/04 §2.6.2, pela ordem que o SDK escreve (§15.1) |
| X3 | ref/04 | registo de "leitura substituída" e registo de "erro de tarefa" | **resolvido:** `NOTE` (tipo 6, ref/04 §3.3); o erro de tarefa vai em `cycle-result.error`, sem registo no buffer (§9.3) |
| X4 | ref/04 | `Err` sem propostas, crashloop sem recriar a instância | **resolvido:** `J-E0816` com `proposals` vazio, tratado como trap sem recriar a instância (ref/04 §2.6.1) |
| X5 | ref/04 | método de classe async com resultado | **resolvido:** `async-io.call` → `list<member-outcome>` (já existia) |
| X6 | ref/04 | receção de caixas no ciclo | **resolvido:** `cycle-io.mailbox-recv` (já existia) |
| X7 | ref/04 | interface de segredos | **resolvido:** `secrets.reveal` (ref/04 §2.5.2) |
| X8 | ref/04 | `run-task` único com subscrição no guest | **substituído:** um `run-task` por disparo, também em `on` e `on_event` (ref/04 §11); §9.2 alinhado |
| X9 | ref/04 | `write` async com o resultado final | **resolvido** (ref/04 §2.4) |
| X10 | ref/04 | relógios do latch e aleatoriedade semeada | **resolvido:** ref/04 §2.7; a semente deriva da geração e não é gravada no traço (§9.1 alinhado) |
| X11 | ref/04 | agregação e qualidade por passo em `history` | **resolvido:** qualidade dominante por passo (ref/04 §2.4); `agg` nos SDKs (§6.2) |
| X12 | ref/04 | variante bloqueante do mundo (plano B) | **resolvido:** mundo `app-blocking` reservado (ref/04 §2.11) |
| X13 | ref/04 | nomes Rust de referência para a paridade | **resolvido:** §16 |
| X14 | ref/09 | `operationId` | **resolvido:** os do OpenAPI (§23); os nomes propostos aqui foram abandonados |
| X15 | ref/09 | `getInfo` com versão e capacidades | **resolvido:** `features` e `getVersion` (ref/09 §5.1) |
| X16 | ref/09 | `dry_run` em escritas e métodos | **substituído:** operações de pré-visualização `previewManualWrite` e `previewMethodCall` (§23) |
| X17 | ref/09 | onde vão `nonce` e `ts`, e o código de recusa | **resolvido:** `Joker-Nonce`, `Joker-Timestamp`, `J-E1021`/`J-E1022` (ref/09 §2.5) |
| X18 | ref/09 | esquema das mensagens do WebSocket e cursor de retoma | **resolvido:** ref/09 §6 (`resume` com `session` e `last_seq`) |
| X19 | ref/09 | Arrow no histórico; exportação em Parquet | **recusado pela ref/09:** NDJSON; Arrow e Parquet do lado do cliente (§23, §26.1) |
| X20 | ref/09 | upload em blocos do *bundle* | **resolvido:** ref/09 §5.13 |
| X21 | ref/02 | resolução de `--node` e identidades | **resolvido:** ref/02 §3.3.1, seguida tal como está (§22) |
| X22 | ref/02 | admissão de tier 2 pelo `max` | **resolvido:** D15, ref/02 §2.1.13 |
| X23 | 17 | nomes de membros reservados | **resolvido:** ref/00 §2 (`name`, `index`, `health`, `read`, `raw`, `has`) |
| X24 | ref/00 | `J-E08xx` também para as bibliotecas cliente | **resolvido:** ref/00 §8 |

---

## 33. Achados nos documentos existentes

Todos foram aceites na integração da rodada (documento 22, §7), com as resoluções da §5 onde havia conflito.

| # | Documento | Problema | Mudança proposta | Estado |
|---|---|---|---|---|
| A1 | 04 §4.1 | `inputs: { zoneTemp … }` em `camelCase`, contra o ref/00 §2; `io.log.info` | `zone_temp`; `import { log }` e `log.info(…)` | aceite e aplicado (documento 22, §7) |
| A2 | 04 §4.2 | `from joker import …`; `def control(self, io)`; `sum(v for v in h) / len(h)` sobre amostras; `io.log` | `from joker_sdk import …`; `class Tank(App)`; `def control(self)` com `self.level.get()`; `h.mean()`; `log.info` | aceite e aplicado (documento 22, §7) |
| A3 | 04 §3.1, §3.3 | a porta chama-se `grid`, e o `bind` do 03 §8 e do exemplo 01 §3 liga `grid_power` (`J0121`/`J0110` no `jk check`) | renomear a porta para `grid_power` no 04 | aceite e aplicado (documento 22, §7) |
| A4 | 04 §3.2 | `plan.limit_kw.min(io.limit.get())` com `get_or_default()`: antes do primeiro `replan` (até 15 min) ou com o plano expirado, o limite é 0 kW e a bateria descarrega para cobrir toda a importação | `let limit = if stale { io.limit.get() } else { plan.limit_kw.min(io.limit.get()) };` (como no §20.1) | aceite e aplicado (documento 22, §7) |
| A5 | 04 §3.3 | `watch_soc` usa `s.value` sem ver a qualidade | `if let Ok(v) = s.get() { … }` | aceite e aplicado (documento 22, §7) |
| A6 | 14 §9.1 | a principal é reescolhida em todos os ticks pela menor contagem de horas: com horas próximas, as bombas trocam de principal com a bomba a trabalhar; `rotate_every` não é usado | escolher a principal no início de cada pedido (como no §20.2) ou usar `rotate_every` | aceite e aplicado (documento 22, §7) |
| A7 | 14 §6.6 | `@joker/classes/std` contradiz o 17 §6 (`@joker/classes-std`); `from joker.classes.std`; `(self, io)`; `cycle("10ms")` abaixo do mínimo de tier 2 | `@joker/classes-std`; `from joker_classes.std`; `def control(self)`; `"100ms"` em TS e `"500ms"` em Python, com a nota do §14.3 | aceite e aplicado (documento 22, §7) |
| A8 | 17 §3.2 | nada impede um membro chamado `name`, `index`, `health`, `read`, `raw` ou `has`, que colide com a API dos handles gerados | reservar estes nomes (código novo na gama `JC`, do 17) | resolvido: ref/00 §2 |
| A9 | 17 §6 | Python `joker.classes.std` | `joker_classes.std` (§3) | aceite e aplicado (documento 22, §7) |
| A10 | 04 §4.3 | "Python na Fase 4" (não há Fase 4: o SDK Python é da 1.2, documentos 11 e 21); "período mínimo recomendado"; "unidades em runtime" | remeter para o §19 deste documento: mínimos como regra (`J-E0854`), memória 64/128 MiB, unidades validadas no `jk build` | aceite e aplicado (documento 22, §7) |
| A11 | 09 §2.2 (último ponto), 01 §9 | "as chamadas `cycle-io.read/write` ficam para os SDKs de tier 2" | os SDKs de tier 2 também usam a imagem empacotada; `cycle-io.read/write` só para acesso dinâmico e para valores que não cabem em `u64` | aceite e aplicado (documento 22, §7) |
| A12 | 01 §9, 09 §2.2 contra 04 §2 | propostas de 12 B (`porta u16, flags u16, valor u64`) contra registos de escrita de 14 B (`kind, flags, porta u32, valor`) | fixar um só layout no ref/04 e corrigir os outros dois | resolvido: um só layout, o da ref/04 §3.3 (registos de 14 B) |
| A13 | 04 §2 | o mundo não tem segredos, métodos de classe com resultado em async, receção de caixas de correio no ciclo, nem os tipos de porta de alarmes e publicados | acrescentar (X1–X7 do §32) | resolvido: X1–X7 do §32 |
| A14 | 01 §9, 03 §8 | `memory.max` por omissão de 16/64 MiB para todas as apps | por omissão dependente do tier (o `joker:meta` traz a sugestão, §19) | aceite e aplicado (documento 22, §7) |
| A15 | 18 §4 (tabela, linha "números dos SDKs") | "Fases 3 e 4" | "Fase 3 (TS) e 1.2 (Python)" | aceite e aplicado (documento 22, §7) |
| A16 | 04 §3.1 | `#[alarm]` só tem `raise()`/`clear()` nos exemplos | acrescentar `set(bool)` no ref/04 para a paridade com o §7.2 | aceite e aplicado (documento 22, §7) |
| A17 | 07 §4 | `jk write … --reason "…"` não diz se o motivo é obrigatório | obrigatório, como nas bibliotecas (§23) | aceite e aplicado (documento 22, §7) |
| A18 | — | a posse da organização `@joker` no npm não foi confirmada (a página devolve 403 a pedidos automáticos) | confirmar antes da primeira publicação; alternativa: `@joker-rt/*` | em aberto: D18 manda confirmar a organização antes da primeira publicação |
