# ref/05 — Tier 2 SDKs (Python, TypeScript) and API client libraries

Normative reference for two surfaces:

- **A. Tier 2 app SDKs**: the API used to write Joker apps in **TypeScript** (ComponentizeJS/jco) and in **Python** (componentize-py), compiled to components of the `joker:runtime` world.
- **B. Local HTTP API client libraries**: `joker-client` in **Rust** (the one `jk` uses), **Python** and **TypeScript**.

It applies `ref/00` without repeating it. The semantics (arbiter, envelope, quality, expressions) are those of document 16, and this document only references them. The decisions that belong to other references (the WIT world and the `joker:meta` of ref/04, the `operationId`s of ref/09, the lock of ref/02) are taken there and followed here; §32 says where each one ended up.

---

## 1. Scope and sources

### 1.1 What it covers

| Part | Surface | Packages | Status |
|---|---|---|---|
| A | TypeScript app SDK | `@joker/sdk`, `@joker/test`, `@joker/classes-<package>` | **v1.0** (Phase 3) |
| A | Python app SDK | `joker-sdk`, `joker-testing`, `joker-classes-<package>` | **v1.2** (documents 11 and 21); the API is closed here |
| B | Rust client | crate `joker-client` | **v1.0** (`jk` is a client of this crate) |
| B | Python client | `joker-client` (PyPI), import `joker_client` | **v1.0** |
| B | TypeScript client | `@joker/client` (npm) | **v1.0** (Node ≥ 20); browser in 1.3, with the Studio |

It does not cover: the WIT world and the Rust SDK (ref/04), the API endpoints and schemas (ref/09), the client configuration file and the contexts (ref/02), the `.jkc` format and what `jkc` generates (document 17).

### 1.2 Sources

| Document | What was taken |
|---|---|
| 04 §1, §2, §3, §4 | DX principles, the WIT world, the reference Rust API, the examples and the tier 2 limits |
| 14 §3, §6, §7 | class handles, `Many<T>`, local classes, active implementations, TS and Python |
| 17 §2, §4, §6, §11 | `method` versus `action`, method expansion, what `jkc` generates, reading `out` |
| 02 §4–§8, §11 | quality, time, read and write semantics, actions, alarms |
| 16 §1, §3, §9 | arbiter, normative table of quality access, code registry |
| 01 §4, §5, §9, §14, §17, §18 | tick, async domain, Wasm host, local API, security, time |
| 07 | `jk` commands that the clients must make it possible to reproduce |
| 09 attacks 2, 6, 8, 10 | packed image, two instances, versions, determinism |
| 20 §2 | local-first: everything the Deck does, a client does node by node |
| 03 §8, §13 | `[apps.*]`, roles and `api.listen` |
| 11, 21 | what is v1.0, 1.1 and 1.2 |

### 1.3 Connections

| Reference | Relation |
|---|---|
| ref/00 | names, time, values, quality, results, errors, limits (applied without exception) |
| ref/04 | each function of the tier 2 SDKs maps 1:1 to a function of the `joker:runtime` world (§16) and to a key of `joker:meta` |
| ref/09 | the client libraries follow the `operationId`s and the schemas of `spec/openapi/joker-api.yaml` (§23) |
| ref/02 | the client libraries read the `jk` client configuration (contexts, identities) with the same rules as `jk` (§22) |
| 17 | the types generated for the classes (§14) |

---

# Part A — Tier 2 app SDKs

## 2. Principles

1. **Semantic parity with the Rust SDK, not syntax parity.** Each concept of the Rust SDK (ref/04) has exactly one equivalent in each language, with the same semantics and the same name translated by the rules of ref/00, §2. The syntax is the idiomatic one of each language.
2. **The code is the app's manifest** (04 §1). The declarations run once, in `jk build`, and produce the `joker:meta` section. There is no `app.toml`, and the `[tool.joker]` and `"joker"` keys only tell `jk build` where the app is (§18).
3. **User names in `snake_case` in every language** (ref/00 §2): ports, parameters, tasks, records, class members and enum variants. Only the SDK API follows the style of the language (`getOr` in TS, `get_or` in Python).
4. **Quality never disappears silently** (invariant 4). The equivalent of Rust's `?` is an **exception** (`QualityError`) that the SDK catches at the task boundary. The equivalent of `panic` is any other exception (§9.3).
5. **Cycle and async are different domains.** In TS, they are different types (`CycleIo<P>` and `TaskIo<P>`), and the compiler catches the errors. In Python, they are different functions (`def` versus `async def`), `jk build` catches the common errors by static analysis (`J-E0859`), and the rest gives `J-E0901`/`J-E0802` at runtime.
6. **Same hot path as Rust.** The tier 2 SDKs use the packed image of `on-cycle` (09 attack 2), and not one host call per read. A host call from a JS or Python engine costs microseconds, and 250 ports would cost milliseconds.
7. **What is tested is what goes to production.** The harness runs the real `.wasm` in wasmtime, with a virtual clock (04 §3.7), and it is the same `joker-test` crate as Rust, wrapped (§17).

---

## 3. Packages, versions and world

| Package | Registry | Import | Contents |
|---|---|---|---|
| `@joker/sdk` | npm | `@joker/sdk` | declarations, types, guest runtime, build tool `joker-build` (Node) |
| `@joker/test` | npm | `@joker/test` | harness with virtual clock (napi-rs native addon over `joker-test`), setup for vitest |
| `@joker/classes-<package>` | npm | same | handles generated by `jkc` (`@joker/classes-std`; `acme/water` → `@joker/classes-acme-water`) |
| `joker-sdk` | PyPI | `joker_sdk` | declarations, types, guest runtime, `python -m joker_sdk.build` |
| `joker-testing` | PyPI | `joker_testing` | harness (PyO3 wheel over `joker-test`), pytest plugin |
| `joker-classes-<package>` | PyPI | `joker_classes.<package>` | generated handles (`joker_classes.std`; `acme/water` → `joker_classes.acme.water`); `joker_classes` is a *namespace package* (PEP 420) |

**Why `joker_sdk` and not `joker`.** The name `joker` exists on PyPI and is the import *namespace* of a set of third-party packages (`joker-stream`, `joker-studio`, …), and `joker-test` also exists (checked on 2026-09-24). An `import joker` in the documentation would lead people to `pip install joker` and to install something else: it is a supply-chain risk, not only an aesthetic one. In Python, the import name is the distribution's with `_`. On npm, `@joker/sdk`, `@joker/client` and `@joker/test` were free on the same date; ownership of the `@joker` organization must be confirmed before the first publication (§33, A18).

**Versions (ref/00 §1).** Each SDK has its own SemVer and declares the world it generates:

| SDK | Generated world | Rule |
|---|---|---|
| `@joker/sdk` 1.y | `joker:runtime@1.0.0` (or the minor that 1.y adds) | a minor of the SDK may start generating the next minor of the world; `joker:meta` says which (`wit`) |
| `joker-sdk` 1.y | same | same |
| 0.x (before 1.0) | `joker:runtime@0.x.y` | the M21 policy (09 attack 8): the runtime accepts the current world and the previous minor |

The `joker:meta` of a tier 2 app is that of ref/04 §5 (`spec/cddl/joker-meta.cddl`), with `sdk-info` (key 4) filled in as follows: `0` = `lang` (`"typescript"` or `"python"`), `1` = SDK version, `2` = backend (0, `wasm`), `3` = `tier` (2), `4` = `engine` (`"starlingmonkey <version>"` or `"cpython <version>"`), `5` = the package (`"@joker/sdk"`, `"joker_sdk"`). `jk check` uses `tier` and `lang` to apply the limits of §19, and key 16 (`limits`) carries the `memory.max` suggestions of §19.

---

## 4. Base types

### 4.1 Signal types in each language

| Signal type (ref/00 §6) | Rust | TypeScript | Python | Notes |
|---|---|---|---|---|
| `bool` | `bool` | `boolean` | `bool` | |
| `i64` | `i64` | `bigint` | `int` (marker `i64` in the declaration) | TS always uses `bigint`: there is never a silent loss above 2⁵³ |
| `u64` | `u64` | `bigint` | `int` (marker `u64` in the declaration) | same; Python validates the range on write (`J-E0806`) |
| `f64` | `f64` | `number` | `float` (or marker `f64`) | default type of physical quantities |
| `f32` | `f32` | `number` | `float` with marker `f32` | promoted to `f64` on read (02 §2) |
| `enum<E>` | generated enum | union of literals with the variant names (`"run"`) | subclass of `joker_sdk.Enum` (`IntEnum`) | TS uses the name, like JSON (ref/00 §4.1); Python uses the member (`Mode.run`) |
| `bits<N>` | generated type | `Bits<Names>` (`.word: bigint`, `.has(name)`) | subclass of `joker_sdk.Bits` (`IntFlag`) | |
| `string` | `String` (≤ 64 B) | `string` | `str` | validated as UTF-8 ≤ 64 B on write (`J-E0805`) |
| `bytes` | `Vec<u8>` (≤ 64 B) | `Uint8Array` | `bytes` | same |
| record (events, arguments, `Internal`) | `#[derive(Record)]` | `record({...})` → `Infer<typeof R>` | class with `@record` | deterministic CBOR, map with **integer keys** = position of the field counting from 0 (D2, ref/00 §4.1, ref/04 §4.10); the names only in `joker:meta` |

The numbers on the ports are **always in the unit declared by the port**. The runtime converts from the signal's unit (02 §3). The SDK APIs do not accept quantity literals (`"50 kW"`): those belong to the text interfaces (ref/00 §4.1), such as the client library.

### 4.2 Time

| Concept | TypeScript | Python |
|---|---|---|
| duration | `Duration` (`.ns: bigint`, `.seconds: number`); the functions accept `DurationLike = string \| Duration` | `Duration` (`.ns: int`, `.seconds: float`); the functions accept `DurationLike = str \| Duration \| datetime.timedelta` |
| instant | `Instant` (`.monoNs: bigint`, `.utcNs: bigint`, `.clock: ClockQuality`) | `Instant` (`.mono_ns: int`, `.utc_ns: int`, `.clock: ClockQuality`) |
| arithmetic | `now.minus("24h")`, `now.plus(d)`, `a.since(b)` | `now - "24h"`, `now + d`, `a - b` → `Duration` |
| UTC | `now.utcSeconds(): bigint`, `now.toDate(): Date` (ms) | `now.utc_seconds() -> UtcSeconds`, `now.datetime() -> datetime` (µs, `tz=UTC`) |

Duration strings follow ref/00, §3.1. An invalid literal is a `TypeError` in the declaration (error in `jk build`) or `J-E0806` at runtime. Instants in nanoseconds are `bigint` in TS because the monotonic clock in ns passes 2⁵³ after ~104 days of operation, and UTC in ns passed it long ago.

### 4.3 Quality and samples

```ts
export type QualityCode = "good" | "uncertain" | "bad";
export type QualityReason =
  | "none" | "overridden" | "local_override" | "stale" | "not_confirmed" | "substituted"
  | "out_of_distribution" | "sensor_cal" | "clock_unsynced" | "initial" | "frozen" | "partial_write"
  | "comm_failure" | "device_fault" | "out_of_range" | "config_error" | "not_connected" | "nan"
  | "tripped" | "deadline" | "unknown";            // "unknown": future reason (ref/00 §1, rule 2)
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

The enum values are the strings of ref/00, §4.3, which are also those of the API JSON: a `Quality` from an app and a `Quality` read by the client have the same shape. A numeric reason between 20 and 255 reaches the SDK as `unknown` (future and private reasons, ref/00 §4.3).

`Sample<T>` / `Sample[T]` (fields in `snake_case` in Python and in `camelCase` in TS, because they are SDK API):

| Field | Type | Meaning (02 §5) |
|---|---|---|
| `value` | `T` | the value delivered by the host. It always exists; with `bad` quality it has no process meaning and only serves for diagnostics |
| `quality` | `Quality` | |
| `ts_src` / `tsSrc` | `int \| None` / `bigint \| null` | UTC ns of the remote device |
| `ts_hw` / `tsHw` | same | monotonic ns of the hardware capture |
| `ts_acq` / `tsAcq` | `int` / `bigint` | monotonic ns of the reception |
| `seq` | `int` / `bigint` | per-signal counter |

And the same accessors as a port: `get()`, `get_uncertain()` / `getUncertain()`, `get_or(d)` / `getOr(d)`, `is_good()` / `isGood()`.

### 4.4 Write results

The shape is that of the JSON of ref/00, §4.4, in both languages:

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
    applied: int | float | None = None      # only in clamped_range / clamped_rate
    level: str | None = None                # only in overridden, "<band>:<level>"
    reason: RejectReason | None = None      # only in rejected
    @property
    def ok(self) -> bool: ...               # accepted, clamped_*, suppressed
```

- `none` (value 0 of ref/00 §4.4, "no proposal in this tick") is not a `WriteOutcome`: `last_outcome()` returns `None`/`null`.
- The WIT distinguishes `clamped(clamp)` (ref/00 §4.4); the SDK translates it into `clamped_range` and `clamped_rate`.
- `overridden(u8)` arrives with the absolute level; the SDK formats it as `"<band>:<level>"` with the bands of document 01, §6.
- A new WIT variant arrives as `unknown` (ref/00 §1, rule 2).
- In Python, `match` works on the `dataclass`: `case WriteOutcome(outcome=Outcome.CLAMPED_RATE, applied=v)`.

### 4.5 Errors

```ts
export declare class JokerError extends Error { readonly code: string; }       // "J-E0800"
export declare class QualityError extends JokerError { readonly port: string; readonly sample: Sample<unknown>; }
export declare class DomainError extends JokerError {}          // J-E0901, J-E0802
export declare class ActionError extends JokerError {
  readonly kind: "denied" | "forbidden" | "timeout" | "device" | "invalid" | "busy" | "not_connected"
               | "cancelled" | "conflict" | "unsupported" | "failed" | "other";   // ActionErrorKind of ref/04 §4.7.1
  readonly detail: string;
}
export declare class CapabilityError extends JokerError { readonly capability: string; }
export declare class LimitError extends JokerError {}           // J-E0804, J-E0805, J-E0806, J-E0811
export declare class UnavailableError extends JokerError {}     // J-E0601 (@unstable item, stub in dev) or J-E0809 (API of a world later than the generated one)
export declare class KvError extends JokerError {}
export declare class HostError extends JokerError {}            // host error with its own code (ref/04)
```

Python has the same hierarchy (`class JokerError(Exception)` with `code: str` and `message: str`, and the subclasses with the same names and fields in `snake_case`). The `str()` of a `JokerError` always starts with the code (`"J-E0800: port 'grid_power' is bad(comm_failure)"`), so that `jk explain` works from any log (ref/00 §8).

### 4.6 Auxiliary declarations

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
// opaque: the SDK builds them and app writers do not use them directly
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
// CycleView<Cy, Req> and TaskView<Ta, Req>: the generated view of a class, with the members of the capabilities
// of Req required (§14.1); LocalCycle/LocalTask: the same for local classes
```

---

## 5. Declaring an app

### 5.1 The choice in each language

| | Rust (ref/04) | TypeScript | Python |
|---|---|---|---|
| app | `#[joker::app(name, version)] struct` | `export default app({ name, version, … })` (**definition object**) | `@app(name=…, version=…)` on `class X(App)` |
| ports | fields with attributes | properties of `inputs`, `outputs`, `params`, … | class attributes (descriptors) |
| cycle task | `#[joker::cycle] fn(io, st)` | `cycles: { name: cycle(opts, (io, st) => …) }` | `@cycle(…) def name(self, st)` |
| async task | `#[joker::task] async fn(io)` | `tasks: { name: task.every(…, async (io) => …) }` | `@task.every(…) async def name(self)` |
| I/O view | `CycleIo<A>` / `TaskIo<A>` | `CycleIo<P>` / `TaskIo<P>` (types inferred from the definition) | `self`, which is the view of the domain where the Wasm instance runs |

**TypeScript: definition object, and not decorators.** With an object, TypeScript infers the type of `io` from the declaration itself: `io.grid_power.get()` is `number`, `io.battery.set("x")` is a type error, and `io.grid_power.history()` only exists in `TaskIo`. With decorators (TC39, TS ≥ 5.0), the fields of a class cannot change the type of `this` per domain, the cycle and async tasks would share `this` (when they are two different instances, 09 attack 6), and inference would require generating code. The object works in JavaScript without a compiler, and it is what document 04 already showed.

**Python: class with decorators, and `self` as the view.** It is the idiomatic form, and the descriptors give types to `self.grid_power.get()` in mypy and pyright without code generation. `self` is **not** state: an assignment `self.x = …` raises `J-E0803`, because the cycle and async run in different Wasm instances and an attribute written in one would never be seen in the other. Cycle state goes into `st` (§9.1), and what is shared goes through `Internal`, `Mailbox` or events (§10). The `@app` decorator fixes the identity and validates; the base class `App` gives the types of the view (`now()`, `dt()`, `kv`, …). A class with `@app` that does not inherit from `App` is `J-E0850`.

### 5.2 Python: declaration API

Normative excerpt of `joker_sdk/__init__.pyi`. All arguments after the type are *keyword-only*.

```python
from __future__ import annotations
from collections.abc import AsyncIterator, Awaitable, Callable, Iterator, Mapping, Sequence
from datetime import timedelta
from typing import Annotated, Any, ClassVar, Generic, Literal, Protocol, Self, TypeVar, overload

T = TypeVar("T"); R = TypeVar("R"); A = TypeVar("A"); S = TypeVar("S")
H = TypeVar("H", bound="ClassHandle")
DurationLike = str | "Duration" | timedelta
Severity = Literal["low", "medium", "high", "critical"]

# ---- type markers (ref/00 §6): they only describe the signal; the handles use int and float
i64 = Annotated[int, "i64"]                    # `int` alone = i64
u64 = Annotated[int, "u64"]
f64 = Annotated[float, "f64"]                  # `float` alone = f64
f32 = Annotated[float, "f32"]
UtcSeconds = Annotated[int, "utc_seconds"]
class Enum(IntEnum): ...                       # user enums: class Mode(Enum): off = 0; run = 2
class Bits(IntFlag): ...                       # bit words: class Status(Bits): alarm_overtemp = 1 << 3

# ---- records (events, action arguments and results, Internal, Mailbox)
def record(cls: type[T]) -> type[T]: ...       # frozen dataclass; fields with type annotations
def field(*, unit: str | None = None, default: Any = ..., default_factory: Callable[[], Any] | None = None) -> Any: ...
def state(cls: type[S] | None = None, *, persist: bool = False) -> Any: ...   # cycle state (§9.1)

# ---- the app
def app(*, name: str, version: str | None = None, description: str | None = None) -> Callable[[type[T]], type[T]]: ...

class App:
    # view common to both domains
    def now(self) -> Instant: ...
    # cycle only
    def dt(self) -> Duration: ...
    # async only
    def snapshot(self, *ports: "Readable[Any]") -> tuple[Sample[Any], ...]: ...
    def subscribe(self, *ports: "Readable[Any]") -> AsyncIterator[Change]: ...
    async def sleep(self, d: DurationLike) -> None: ...
    async def sleep_until(self, t: Instant) -> None: ...
    kv: ClassVar["Kv"]
    secrets: ClassVar["Secrets"]

# ---- ports (layer 1)
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

class Action(Generic[A, R]):                     # port bound to an action of a device (02 §8)
    def __init__(self, args: type[A] | None = None, result: type[R] | None = None, *,
                 optional: bool = False, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> ActionHandle[A, R]: ...

class Alarm:
    def __init__(self, *, severity: Severity, message: str, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> AlarmHandle: ...

class Published(Generic[T]):                     # signal app.<app>.<name> (04 §3.1)
    def __init__(self, type: type[T], *, unit: str | None = None, quantity: str | None = None,
                 doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> PublishedHandle[T]: ...

class Counter:                                   # published monotonic counter (u64)
    def __init__(self, *, doc: str | None = None) -> None: ...
    def __get__(self, obj: App, owner: type) -> CounterHandle: ...

class Secret:                                    # declared secret (ref/04 §2.5.2; joker:meta key 13)
    def __init__(self, name: str, *, optional: bool = False, doc: str | None = None) -> None: ...

class Model:                                     # v1.1 (ML); reserved
    def __init__(self, *, inputs: str, outputs: str, doc: str | None = None) -> None: ...

# ---- classes (layer 2, §14)
def device(cls: type[H], *, requires: Sequence[str] = (), optional: bool = False) -> H: ...
def devices(cls: type[H], min: int, max: int, *, requires: Sequence[str] = ()) -> Many[H]: ...
def implements(cls: type[ClassHandle], *, capabilities: Sequence[str] = ()) -> Callable[[type[T]], type[T]]: ...
def local_class(cls: type[T]) -> type[T]: ...

# ---- tasks (§9)
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

def on_action(name: str) -> Callable[[Callable[..., Awaitable[Any]]], Any]: ...     # only with @implements
def on_params(fn: Callable[[Any, Mapping[str, Any]], None]) -> Any: ...
def migrate(fn: Callable[[Any, Any, str], Any]) -> Any: ...                         # (self, previous, from_version) -> state
```

### 5.3 TypeScript: declaration API

Normative excerpt of `@joker/sdk/index.d.ts`. `P` is the type of the ports inferred from the definition.

```ts
export type DurationLiteral = `${number}${"ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"}`
                            | `${number} ${"ns" | "us" | "µs" | "ms" | "s" | "m" | "min" | "h" | "d"}`;
export type DurationLike = DurationLiteral | Duration;
export type Severity = "low" | "medium" | "high" | "critical";

// ---- value schemas
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

// ---- port declarations (layer 1)
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
export declare function model(o: { inputs: string; outputs: string }): ModelDecl;  // v1.1, reserved

// ---- classes (layer 2, §14)
export interface ClassSchema<Cy, Ta, Caps extends string> { readonly class: string; readonly __cy?: Cy; readonly __ta?: Ta; readonly __caps?: Caps }
export interface DeviceDecl<C, Req extends string> { readonly kind: "device"; readonly __c?: C; readonly __r?: Req }
export interface ManyDecl<C, Req extends string> { readonly kind: "many"; readonly __c?: C; readonly __r?: Req }
export declare function device<Cy, Ta, Caps extends string, Req extends Caps = never>(c: ClassSchema<Cy, Ta, Caps>, o?: { requires?: readonly Req[]; optional?: boolean }): DeviceDecl<ClassSchema<Cy, Ta, Caps>, Req>;
export declare function many<Cy, Ta, Caps extends string, Req extends Caps = never>(c: ClassSchema<Cy, Ta, Caps>, o: { min: number; max: number; requires?: readonly Req[] }): ManyDecl<ClassSchema<Cy, Ta, Caps>, Req>;
export declare function implementsClass<Cy, Ta, Caps extends string>(c: ClassSchema<Cy, Ta, Caps>, o?: { capabilities?: readonly Caps[] }): ImplementsDecl<ClassSchema<Cy, Ta, Caps>>;
export declare function localClass<L extends LocalClassDef>(def: L): ClassSchema<LocalCycle<L>, LocalTask<L>, never>;

// ---- tasks (§9)
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

// ---- the app
export interface AppDef<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta> {
  name: string; version?: string; description?: string;
  inputs?: I; outputs?: O; params?: Pa; internals?: In; mailboxes?: Mb; alarms?: Al;
  published?: Pu; devices?: Dv; actions?: Ac; secrets?: readonly SecretDecl[];
  models?: Readonly<Record<string, ModelDecl>>;                             // v1.1
  implements?: ImplementsDecl<any>;
  cycles?: Cy; tasks?: Ta;
  onAction?: Record<string, (io: TaskIo<any>, args: any, cx: ActionCx) => Promise<any>>;   // only with implements
  onParams?: (params: Readonly<Record<string, unknown>>) => void;
  migrate?: (previous: unknown, fromVersion: string) => unknown;
}
export declare function app<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta>(def: AppDef<I, O, Pa, In, Mb, Al, Pu, Dv, Ac, Cy, Ta>): AppComponent;
```

The tasks receive `io` with the type `CycleIo<P>` or `TaskIo<P>`, where `P` gathers all the ports of the definition (the keys of `inputs`, `outputs`, `params`, `devices`, … live in a single namespace, and a key repeated in two groups is `J-E0857`). In TS the tasks are written inside `app({...})` so that inference works; a task defined outside must annotate `io` with `CycleIo<typeof ports>`, and the SDK exports `defineCycle(ports, …)` and `defineTask(ports, …)` for that.

### 5.4 Table of declarations

The last column says where the declaration goes in `joker:meta` (ref/04 §5; the keys are integers, and `port.1` is the `port-kind`).

| Concept | Rust (ref/04) | TypeScript | Python | `joker:meta` |
|---|---|---|---|---|
| app | `#[joker::app(name, version)]` | `app({ name, version })` | `@app(name=, version=)` | keys 1 and 2 |
| input | `#[input(unit, quantity)] x: Input<f64>` | `inputs: { x: input.f64({ unit, quantity }) }` | `x = Input(float, unit=, quantity=)` | port, `1: 0` (input) |
| multiple input | `#[input(many)] InputMany<T>` | `inputs: { x: inputMany.f64(...) }` | `InputMany(float, ...)` | port with `6` (`many`) |
| output | `#[output] Output<T>` | `outputs: { x: output.f64(...) }` | `Output(float, ...)` | port, `1: 1` (output) |
| multiple output | `#[output(many)] OutputMany<T>` | `outputMany.f64(...)` | `OutputMany(float, ...)` | same, with `6` |
| parameter | `#[param(unit, range, default)] Param<T>` | `params: { x: param.f64({ unit, range, default }) }` | `Param(float, unit=, range=, default=)` | port, `1: 2`, with `7` (`range`) and `8` (`default`) |
| internal signal | `#[internal] Internal<R>` | `internals: { x: internal(R) }` | `Internal(R)` | port, `1: 3`, with `9` (schema) and `11` (domain) |
| mailbox | `#[mailbox(to)] Mailbox<R, N>` | `mailboxes: { x: mailbox(R, { capacity, to }) }` | `Mailbox(R, capacity=, to=)` | port, `1: 9`, with `12` |
| input event | `#[input(event)] EventInput<R>` | `inputs: { x: input.event(R) }` | `EventInput(R)` | port, `1: 6` |
| output event | `#[output(event)] EventOutput<R>` | `outputs: { x: output.event(R) }` | `EventOutput(R)` | port, `1: 7` |
| action (use) | `#[action] Action<A, R>` | `actions: { x: action({ args, result }) }` | `Action(A, R)` | port, `1: 8`, with `13` |
| alarm | `#[alarm(severity, message)] Alarm` | `alarms: { x: alarm({ severity, message }) }` | `Alarm(severity=, message=)` | port, `1: 10`, with `14`; id `app.<app>.<name>` |
| published | `#[publish] Published<T>` / `Counter` | `published: { x: published(output.f64(...)), n: counter() }` | `Published(float, ...)` / `Counter()` | port, `1: 4` / `1: 5` |
| secret | `#[joker::app(secrets = [...], optional_secrets = [...])]` | `secrets: [secret("mqtt_password")]` | `Secret("mqtt_password")` | key 13 |
| model (1.1) | `#[model] Model` | `model({...})` | `Model(...)` | port, `1: 11`; key 10 with `"ml"` |
| equipment | `#[device(requires)] Pump` | `devices: { p: device(Pump, { requires }) }` | `p = device(Pump, requires=)` | key 8 |
| several pieces of equipment | `Many<Pump, 1, 8>` | `many(Pump, { min: 1, max: 8 })` | `devices(Pump, 1, 8)` | key 8, with `4` (`many`) |
| implement a class | `#[joker::implements(Pump, capabilities)]` | `implements: implementsClass(Pump, { capabilities })` | `@implements(Pump, capabilities=)` | key 9, with the contract hash |
| local class | `#[derive(joker::Class)]` | `localClass({...})` | `@local_class` | key 8, `class-ref = "local:<Name>"` |
| cycle task | `#[joker::cycle(period, wcet, init)]` | `cycle({ period, wcet, state }, fn)` | `@cycle(period, wcet=, state=, init=)` | key 7, `1: 0` |
| async task | `#[joker::task(every/on/on_event/at/startup)]` | `task.every/on/onEvent/at/startup` | `@task.every/on/on_event/at/startup` | key 7, `1: 1`, with `6` (`trigger`) |
| persistent state | `#[joker::state(persist)]` | `cycle({ persist: true })` | `@state(persist=True)` | key 14 |
| actions of a virtual instance | `#[joker::on_action(<action>)]` | `onAction: { reset: … }` | `@on_action("reset")` | key 7, `1: 2`, with `7` |

### 5.5 Validation of the declarations

Done by the SDK while the declaration runs in `jk build` (§18.4). All errors appear before there is a `.wasm`.

| Rule | Code |
|---|---|
| name of the app, port, task, parameter, record field, variant: grammar `[a-z][a-z0-9_]*`, ≤ 32 characters (02 §9, 16 P5) | `J-E0857` |
| reserved name for a port or task: `now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta` | `J-E0857` |
| two elements with the same name in the app (ports, tasks, equipment, alarms share one space) | `J-E0857` |
| unit outside the catalog (16 §5) | `J0109` |
| quantity (`quantity`) without a unit of the right dimension | `J0108` |
| `range` with `min > max`, `default` outside the `range` | `J-E0851` |
| `Internal` with a non-scalar field, or maximum CBOR size > 64 B (ref/00 §10) | `J-E0804` |
| event or argument record with an unbounded `string` > 1 KiB or a list > 64 (ref/00 §10) | `J-E0805` |
| `@cycle` on `async def`, `@task.*` on `def`, wrong signature, `task.on` on a port that is not an input | `J-E0813` |
| `version` in the code different from that of `pyproject.toml`/`package.json` | `J-E0852` |
| `@app` without inheriting from `App`; zero or more than one app in the entry point | `J-E0850` |
| `@on_action` without `@implements`, or for an action the class does not have | `J-E0813` |
| host call during the declaration (reading the clock, `kv`, a port) | `J-E0810` |

---

## 6. Ports: reading and quality

### 6.1 Accessors

The accessors follow the normative table of document 16, §3.1, without exceptions:

| Python | TypeScript | `good` | `uncertain` | `bad` | Domains |
|---|---|---|---|---|---|
| `get()` | `get()` | value | `QualityError` | `QualityError` | cycle, async |
| `get_uncertain()` | `getUncertain()` | value | value | `QualityError` | cycle, async |
| `get_or(d)` | `getOr(d)` | value | `d` | `d` | cycle, async |
| `sample()` | `sample()` | sample | sample | sample | cycle, async |
| — | `tryGet()` | `{ ok: true, value }` | `{ ok: false, error }` | `{ ok: false, error }` | cycle, async |

- **In the cycle**, all of them read from the tick's latch: two reads in the same tick give the same sample (02 §6.1). The SDK decodes the packed image that `on-cycle` receives and does not call the host.
- **In async**, they read the current slot (`async-io.read`), and two consecutive reads may differ (02 §6.4). To read several signals coherently, `self.snapshot(a, b)` / `io.snapshot([io.a, io.b])`.
- `get_or` records in the trace that the read was substituted (16 §3.1). In the cycle, the SDK appends a `NOTE` record (type 6, ref/04 §3.3) to the proposal buffer; in async, it calls `async-io.note-substituted`.
- `tryGet()` only exists in TS: it is Rust's `Result` for those who prefer not to use exceptions. In Python there is no equivalent, because the exception is the idiomatic form and a second path would only confuse.
- `QualityError` carries the port and the complete sample; `str(e)` is `"J-E0800: port 'soc' is bad(comm_failure)"`.

### 6.2 Input, parameter and internal handles

```python
class InputHandle(Generic[T]):
    def get(self) -> T: ...
    def get_uncertain(self) -> T: ...
    def get_or(self, default: T) -> T: ...
    def sample(self) -> Sample[T]: ...
    # async only
    async def history(self, start: Instant | str | None = None, end: Instant | str | None = None, *,
                      last: DurationLike | None = None, step: DurationLike | None = None,
                      agg: Agg = "mean") -> History[T]: ...

Agg = Literal["raw", "mean", "min", "max", "first", "last", "count"]   # history-query.agg (ref/04 §2.4)

class ParamHandle(Generic[T]):
    def get(self) -> T: ...                           # parameters are always valid: the range belongs to jk check (J0120)

class InternalHandle(Generic[R]):
    def get(self) -> R: ...                           # QualityError while it is uncertain(initial)
    def get_or(self, default: R) -> R: ...
    def get_or_default(self) -> R: ...                # the record with the zero value of each field (§6.2)
    def sample(self) -> Sample[R]: ...
    def set(self, value: R) -> None: ...              # cycle and async; no result

class History(Sequence[Sample[T]]):
    def values(self, *, quality: Literal["good", "uncertain"] = "good") -> list[T]: ...   # only the samples with that quality or better
    def mean(self) -> float: ...                      # mean of the good samples; QualityError if there are none
    def coverage(self) -> float: ...                  # fraction of good samples, 0..1
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

`get_or_default()` returns the record with the **zero value** of each field (`False`, `0`, `0.0`, `""`, `b""`, the variant with value 0 of an enum, `None` in the optional ones), like Rust's derived `Default`, and not the default values of the Python constructor: that way the three languages give the same record.

`history()` returns the historian's samples in the interval (`async-io.history`, ref/04 §2.4). With `step`, one sample per step, aggregated by `agg` (default `mean`), with the **dominant quality** of the step; without `step`, `agg` must be `raw` and the raw points come back. `last="24h"` is `[now − 24h, now]`. `start`/`end` as a string accept RFC 3339 (ref/00 §3). More than 10,000 points is `J-E0402`.

### 6.3 Multiple ports

`InputMany`/`OutputMany` give a `ManyPorts[H]` (Python) or `ManyPorts<H>` (TS): an immutable sequence of handles, ordered by the path of the bound signal, with the size fixed at `init` (04 §3.1). Each element has `.path` (the bound path, as text) and `.index`.

---

## 7. Writes and results

### 7.1 Outputs

| Python | TypeScript | Domain | Semantics |
|---|---|---|---|
| `set(v)` | `set(v)` | cycle | proposal in the tick's buffer; the last one of the tick wins (16 A4); no result |
| `set(v)` | `set(v)` | async | proposal without waiting for the result (`async-io.set`): it goes to the owning group's inbox; `last_outcome()` gives `deferred` until the commit |
| `relinquish()` | `relinquish()` | cycle, async | releases the level immediately (01 §6) |
| `pending()` | `pending()` | cycle | this app's proposal in this tick, or `None`/`null`; also `None` for outputs written only by methods (17 §4.1) |
| `last_outcome()` | `lastOutcome()` | cycle, async | the `WriteOutcome` of this writer's proposal in the most recent commit of the owning group, or `None`/`null` (02 §7.1) |
| `get()`, `sample()` | `get()`, `sample()` | cycle, async | the **applied** value (17 §11), with quality |
| `await write(v)` | `await write(v)` | async | proposal to the owning group; resolves with the **final** result, after the commit (01 §5) |
| `set_at(v, at)` | `setAt(v, at)` | cycle | scheduled write (M6), **v1.1** |
| `await write_at(v, at)` | `await writeAt(v, at)` | async | same, **v1.1** |
| `await propose(v, reason=, ttl=)` | `await propose(v, { reason, ttl })` | async | intent (M18), **v1.1**; returns the `IntentId` (ULID) |

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

- **A bad result is not an exception.** `rejected`, `tripped` and `overridden` are normal results, as in Rust. `outcome.ok` says whether some value of this proposal was applied.
- **Invalid values are exceptions.** A value outside the type (negative `u64`, `i64` above 2⁶³−1, `string` with more than 64 B, non-integer `number` on an `i64` port) raises `LimitError` (`J-E0806`, `J-E0805`) **before** reaching the buffer. A `NaN` or infinity on an `f64` output is a valid value for the SDK, and it is the envelope that handles it (step 1, `Rejected(WrongType)`).
- **In async, forgetting the `await`** of `write()` does not lose the write: the proposal is sent at the moment of the call, and the `await` only waits for the result. Python warns through the normal `RuntimeWarning` of a coroutine that was never awaited; TS through the `no-floating-promises` rule that the `jk app new` template turns on.
- The v1.1 APIs exist in the types since 1.0 and behave **as in the Rust SDK** (ref/04 §2.9): the SDK marks the unstable feature in `joker:meta` (key 10) when the function is referenced (§18.4); in `mode = "prod"`, `jokerd` rejects the component (`J-E0003`, `error-in-prod`), and `jk check` warns before the deploy; in `mode = "dev"`, the runtime binds it to stubs that return `J-E0601`, and the SDK raises `UnavailableError` with that code. `J-E0809` is kept for another case: a **stable** API of a world later than the one the app was generated for (E10).

### 7.2 Published signals, counters and alarms

```python
class PublishedHandle(Generic[T]):
    def set(self, value: T) -> None: ...             # cycle and async

class CounterHandle:
    def add(self, n: int = 1) -> None: ...           # counter-add; n ≥ 0; n < 0 → LimitError (J-E0806)

class AlarmHandle:
    def raise_(self) -> None: ...                    # PEP 8: suffix _ for reserved words
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

An app alarm is a **condition** that the app writes; the lifecycle (delays, acknowledgment, shelving, suppression) is that of the core (02 §11). `raise_()`/`raise()` and `clear()` are sugar over `set(True)`/`set(False)`. In the WIT world, the alarm is an `alarm` port (`port-kind` 10), written with `publish` and a `bool`; `state()` reads the signal `alarm.app.<app>.<name>.state` (from the latch in the cycle, from the slot in async). The alarm id in the API and in `jk` is `app.<app>.<name>` (ref/04 §4.2). Rust has no `get()` on a counter, and so the tier 2 SDKs do not either: whoever needs the value reads the published signal through an input port.

---

## 8. Parameters

- `ParamHandle.get()` always returns a value: the parameters come from the manifest (`[apps.<a>.params]`), with the unit converted to that of the port, the range checked in `jk check` (`J0120`) and the declaration's `default` when the manifest does not give one. There is no quality.
- A parameter without a `default` and without a value in the manifest is `J0142` in `jk check`.
- **Hot change** (`on-params`): the new values take effect from the next tick. If the app declares `@on_params` / `onParams`, the SDK calls it before accepting the values; an exception rejects them (`on-params` returns `Err` with the message, and the generation is rejected for this app with `J-E0011`, ref/04 §2.6.1).
- `Param(float, unit="%", range=(0, 50))` stores the value in `%`. For a fraction, divide by 100 in the code, as in document 04, §3.2.

---

## 9. Tasks

### 9.1 Cycle tasks

```python
@state(persist=True)                       # optional; without persist, the state does not survive the hot swap
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

| Option | Type | Default | Semantics |
|---|---|---|---|
| `period` | duration | required | **requested** period; the group comes from the manifest (`[apps.<a>.cycles]`), and a divergence > `period_tolerance` is `J0130` (04 §3.2). Minimum in tier 2: §19 (`J-E0854`) |
| `wcet` | duration | 20% of the `period` | WCET declared for admission (M23), until `jk bench` measures it; > 50% of the period is `J-E0854` |
| `state` (Python: class; TS: factory) | — | none | the local state of the task; it lives in the memory of the cycle instance |
| `init` (Python) | `(self) -> S` | `S()` | runs once, inside the task's first `on-cycle` (with latch and parameters available); in TS, the factory `state(io)` does the same |
| `persist` | `bool` | `false` | exports the state in `export-state` and restores it in `migrate` (§9.4) |
| `optional` | `bool` | `false` | a task that `overrun = "degrade"` may switch off (01 §4) |

Rules:

- **There is no `async` and no I/O in the cycle.** In Python, a cycle task is a `def`; in TS, the function returns `void`, and a returned `Promise` is `J-E0813` at build (and `J-E0901` at runtime, if it gets through via `any`).
- **State:** `st` is the same object in every tick. In Python, `self` is read-only (`J-E0803`). In TS, `io` is frozen (`Object.freeze`), and an assignment in strict mode throws `TypeError` with the message of `J-E0803`.
- **Deterministic clock and randomness.** In the cycle instance, `Date.now()`, `performance.now()`, `time.time()`, `time.monotonic()` and `time.time_ns()` return the latch instant (the same as `now()`), because the host serves `wasi:clocks` with it (ref/04 §2.7). `Math.random()` and the `random` module are seeded at `init` from `wasi:random`, which the host serves with the deterministic generator seeded by the generation (ref/04 §2.7: it is not recorded in the trace, it is reproduced from the `generation-hash`). The replay is exact.
- **Memory:** the SDK does not reset the memory at the end of the tick (there is GC). The instance's heap is preallocated with `memory.max` and pinned (01 §9); GC pauses happen inside the tick and count towards the WCET (§19).

### 9.2 Async tasks

| Trigger | Python | TypeScript | Handler | Semantics |
|---|---|---|---|---|
| periodic | `@task.every("15m", jitter="30s")` | `task.every("15m", { jitter: "30s" }, fn)` | `(self)` / `(io)` | if the previous execution is still running, the firing stays pending (only one) and the following ones are counted in `joker.app.<app>.task.<t>.coalesced` (ref/04 §4.3.2) |
| cron | `@task.at("0 */15 * * * *")` | `task.at("0 */15 * * * *", fn)` | same | 6-field cron, UTC; requires a `synced` or `holdover` clock, and with `unsynced` the firing waits (ref/04 §4.3.2) |
| change | `@task.on("soc")` | `task.on("soc", fn)` | `(self, s: Sample[T])` / `(io, s)` | one call per new sample; with the handler busy, only the most recent stays (*latest-wins*) and the others are counted as dropped |
| event | `@task.on_event("detections")` | `task.onEvent("detections", fn)` | `(self, e: R)` / `(io, e)` | every event is delivered, in order; the queue is the consumer's (02 §1) |
| startup | `@task.startup()` | `task.startup(fn)` | `(self)` / `(io)` | once, after `init` |

- The async tasks of an app run **concurrently** on the same *event loop* (asyncio in Python, the engine's *event loop* in TS), one at a time between two `await`s. There are no threads (`threading.Thread.start()` and `Worker` fail).
- **How the triggers reach the guest** (ref/04 §2.6.1, §4.3.2): the host calls `run-task(task, why)` once per firing, for every trigger: `timer` for `every` and `at`, `change` for `on`, `event` for `on_event`, `startup`. The SDK dispatches to the handler with the sample or the event already decoded. A firing that arrives while the handler is running stays pending (only one; changes merge into the most recent) and the dropped ones are counted in `joker.app.<app>.task.<t>.coalesced`. The functions `subscribe()` and `events()` remain available for whoever wants to consume a stream inside a task.
- In the async instance, the clocks of `Date.now()` and `time.time()` are the real ones, and so is `now()`. The async replay is that of the proposals at the boundary, or exact with `trace.async = true` (09 attack 10).

### 9.3 Errors in a task

| What happens | Cycle | Async |
|---|---|---|
| the task ends normally | the tick's proposals go on to the arbiter | — |
| a `JokerError` leaves the task (including `QualityError`) | the task ends **in this tick**; the proposals already made **remain** (`cycle-result.error` with the code, ref/04 §2.6.1); it is counted in `joker.app.<a>.task.<t>.errors` and goes to the log with the code | the execution ends (`run-task` returns `Err`); it goes to the log; the trigger continues |
| any other exception | equivalent to a `panic`: this app's proposals **in this tick are discarded** (01 §9); it counts towards the K traps of the crashloop; the log carries the *traceback* | the execution ends; it is counted in `joker.app.<a>.task.<t>.failures`; the log carries the *traceback*; the instance continues |
| real Wasm trap (`memory.grow` fails, stack, epoch deadline) | trap (01 §9) | trap; the async instance is recreated |

The distinction is Rust's: a `JokerError` is an expected `Err` (bad data), and the rest is a bug. An uncaught exception does **not** trap the instance: the SDK catches it and returns `cycle-result` with empty `proposals` and `error.code = J-E0816` (or `Err(J-E0816)` in `run-task`); the host counts it as a trap for the crashloop, without recreating the instance, because recreating a Python instance costs hundreds of ms (ref/04 §2.6.1). For `JokerError`s in the cycle, the SDK returns the tick's proposals and the error in the same `cycle-result`: there is no error record in the buffer.

### 9.4 Persistent state and migration

- With `persist`, the SDK encodes the state of each cycle task in deterministic CBOR in `export-state`, in a map `{ "<task>": state }`, and the `Many`s by instance name (14 A10). Maximum size: 64 KiB (`J-E0815` above it).
- In `migrate(previous, from_version)`, the SDK decodes the map and, if the app declares `@migrate`/`migrate`, calls it with the previous state (already decoded into a `dict`/object) and the version; the returned value is the new state. Without a hook, the SDK restores the fields with the same name and type and uses the default value in the others. A field with an incompatible type and no hook is `J-E0815`, and the new version does not go in (01 §9, "if `init` fails, the old one continues").
- The fields of a persistent state must be encodable in CBOR: scalars, `str`/`string`, `bytes`, lists, maps with text keys and records. An arbitrary object is `J-E0815` at the first `export-state` and, if the SDK can see it through the annotations, already at build.

---

## 10. Sharing between cycle and async

The two instances do not share memory (01 §5, 09 attack 6). The three channels are those of document 04, §3.4:

| Channel | Python | TypeScript | Semantics |
|---|---|---|---|
| internal signal | `Internal(R)` → `get()`, `set(r)` | `internal(R)` → `get()`, `set(r)` | last value, ≤ 64 B, seqlock; the cycle reads from the latch |
| mailbox | `Mailbox(R, capacity=N, to="async")` | `mailbox(R, { capacity: N, to: "async" })` | bounded SPSC queue (`mailbox-send`, `mailbox-recv`, `mailbox-next`, ref/04 §2.3–§2.4); `try_send`/`trySend` never blocks and returns `False` if it is full; the drops are counted by the sender |
| events | `EventOutput(R).emit(r)` | `output.event(R)` → `emit(r)` | queue per consumer |

```python
class MailboxHandle(Generic[R]):
    def try_send(self, item: R) -> bool: ...          # in the sending domain (mailbox-send)
    def lost(self) -> int: ...                        # in the sending domain: sends refused because the queue was full
    def try_recv(self) -> R | None: ...               # in the receiving domain (mailbox-recv)
    def drain(self) -> list[R]: ...                   # same, mailbox-recv until empty
    async def recv(self) -> R: ...                    # only when the receiver is async (mailbox-next)

class EventOutputHandle(Generic[R]):
    def emit(self, event: R) -> None: ...             # cycle and async

class EventInputHandle(Generic[R]):
    def events(self) -> AsyncIterator[R]: ...         # async only
```

```ts
export interface MailboxSend<R> { trySend(item: R): boolean; lost(): bigint }
export interface MailboxRecvCycle<R> { tryRecv(): R | null; drain(): R[] }
export interface MailboxRecvTask<R> extends MailboxRecvCycle<R> { recv(): Promise<R> }
export interface EventOutputHandle<R> { emit(e: R): void }
export interface EventInputHandle<R> { events(): AsyncIterable<R> }
```

In TS the type of `io.<mailbox>` depends on the domain and on `to`: on the sending side there is only `trySend`, on the receiving side there are only `tryRecv`/`drain`/`recv`. In Python the handle has both sides, and the one of the wrong domain raises `DomainError` (`J-E0802`).

**Module global variables.** The same module is loaded in both instances, and a global variable written in an async task is never seen by the cycle (nor the other way round). The SDK cannot prevent it at runtime. `jk build` warns (`J-E0858`) when a function decorated as a task writes to a module global (`global x` in Python; assignment to a module `let` in TS).

---

## 11. Actions

**Using an action** (an `Action` port, or the action of a class instance, §14): only in async, because it waits for a reply (02 §8).

```python
class ActionHandle(Generic[A, R]):
    async def invoke(self, args: A | None = None, *, key: str | None = None,
                     timeout: DurationLike | None = None) -> R: ...                      # async-io.invoke
    def invoke_long(self, args: A | None = None, *, key: str | None = None) -> LongAction[R]: ...  # start-action

class LongAction(Generic[R]):                         # action-call resource (ref/04 §2.4)
    def progress(self) -> AsyncIterator[tuple[float | None, str]]: ...
    def cancel(self) -> None: ...
    async def finish(self) -> R: ...                  # ActionError(kind="cancelled") if it was canceled
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

- `key` is the idempotency key (ref/00 §5: printable ASCII, 1–64 B; `J-E0805` outside that). Without `key`, the SDK generates one (`"<app>:<port>:<ULID>"`) per call. For a retry after a `timeout`, the app reuses the same key, and the core returns the previous result within the `dedup_window` (16 E9).
- `timeout` may only shorten the one in the manifest; it never lengthens it.
- Errors: `ActionError` with the `code` the host gives (`J-E0301`…`J-E0313`) and `kind` derived from it by the table of ref/04 §7 (`denied`, `forbidden`, `timeout`, `device`, `invalid`, `busy`, `not_connected`, `cancelled`, `conflict`, `unsupported`, `failed`, `other`).
- In the cycle, actions do not exist in the TS types, and in Python they raise `DomainError` (`J-E0901`) and are caught by `J-E0859` at build. A cycle task that needs an action publishes an event or uses a `Mailbox`, and an async task executes it (02 §8).
- Long actions (`@long`, 17 §4.2): `invoke_long()`/`invokeLong()` over `async-io.start-action` and the `action-call` resource (ref/04 §2.4). `invoke_long` on an action that is not long is `ActionError` with `J-E0313`.

**Implementing an action** (virtual instance, 17 §5.3): §14.5.

---

## 12. Clock, `kv`, log, secrets and network

### 12.1 `kv` (async only)

```python
class Kv:
    def get(self, key: str) -> bytes | None: ...
    async def set(self, key: str, value: bytes) -> None: ...
    async def delete(self, key: str) -> None: ...
    def keys(self, prefix: str = "") -> list[str]: ...
    def batch(self) -> KvBatch: ...                   # .set(k, v).delete(k); await .commit() (kv.commit, ≤ 64 operations)
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

| Rule | Value | Code |
|---|---|---|
| key | 1–128 B, `[A-Za-z0-9._:/-]` | `J-E0811` |
| value | ≤ 64 KiB | `J-E0811` |
| total per app | ≤ 1 MiB (01 §9.1) | the host refuses: `KvError` with `J-E0404` (ref/04 §2.5.1) |
| in the cycle | does not exist (01 §9.1) | `J-E0901` |

`get` is synchronous because the WIT is (04 §2); `set` and `delete` only resolve after the fsync of the `kv` policy. Records use the same CBOR as events (§15.2).

### 12.2 Log

```python
from joker_sdk import log
log.info("new plan", target_soc=0.62, source="history")     # also trace, debug, warn, error
```

```ts
import { log } from "@joker/sdk";
log.info("new plan", { target_soc: 0.62, source: "history" });
```

- The fields go as `list<tuple<string, string>>` (`log.log`, ref/04 §2.5.3): the SDK converts each value with `str()`/`String()`. The message is truncated to 1 KiB and the fields to 16, with the mark `…` at the end (no error).
- `log.enabled("debug")` / `log.enabled("debug")` is the WIT's `log.enabled`: the SDK uses it so as not to format messages the host would throw away.
- It works in both domains; in the cycle it goes to the lock-free ring (09 attack 5).
- In Python, the SDK installs a handler in `logging` (the libraries' `logging.getLogger(__name__)` end up in the same place) and `print()` goes to the log with `stream=stdout`. In TS, `console.log/info/warn/error` do the same.
- The field names are user data: they stay in `snake_case`, even in TS.

### 12.3 Secrets

A secret is declared by its local name (`Secret("mqtt_password")`, `secret("mqtt_password")`; key 13 of `joker:meta`), bound in the manifest in `[apps.<app>.secrets] mqtt_password = "secret:<name>"` (D6, ref/01) and read only in async: `await self.secrets.get("mqtt_password")` / `await io.secrets.get("mqtt_password")`, over `secrets.reveal` (ref/04 §2.5.2). It returns a `SecretStr` (Python) or `SecretString` (TS) whose `repr`/`toString`/`toJSON` is `"***"`; the value is read with `.reveal()`. Reading a name the app did not declare is `J-E0817`, without calling the host; declared but not bound is `J-E0408`, and bound but absent on the node is `J-E0409` (ref/04). The manifest never has the value (01 §17, ref/00 §9).

### 12.4 Network (async only)

- **TS:** global `fetch()`, implemented over `wasi:http/outgoing-handler`. It only reaches the destinations in `[apps.<a>.net] allow` (03 §8); the rest fails with a `fetch` `TypeError` whose cause is a `HostError` with the host's code. In the cycle, `fetch` throws `DomainError` (`J-E0901`).
- **Python:** `joker_sdk.http` (`async def request(method, url, *, headers=None, body=None, timeout=None) -> Response`, plus `get` and `post`), over the same `wasi:http`. `socket`, `urllib.request`, `requests` and `httpx` do not work, because there are no sockets in the component.
- There is no file system. The data files declared in `include` (§18) are read with `importlib.resources` (Python) or `import … with { type: "json" }` (TS, embedded by the bundler).

---

## 13. The I/O view

**Python.** `self`, inside a task, is the view of the domain in which the instance runs. The class attributes (descriptors) return handles; the methods of `App` are in §5.2. Calling a function of the other domain raises `DomainError`: `J-E0901` (async API in the cycle) or `J-E0802` (cycle API in async, such as `dt()` or `pending()`).

**TypeScript.** `io` is a frozen object with the ports of the definition and the services of the domain:

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
  never;                                   // EventInput and Action do not exist in the cycle
type TaskHandleOf<D> = /* the same, with InputTask, OutputTask, MailboxRecvTask, EventInputHandle,
                          ActionHandle and TaskView of the classes */ unknown;
```

A `never` in the cycle type is what makes `io.reset_line.invoke()` in a cycle task a compile error, as A9 of document 14 asks.

---

## 14. Equipment classes

### 14.1 What `jkc` generates

For each class, `jkc` (17 §6) generates one module per language. For `std/pump@1`:

```python
# joker_classes/std/pump.pyi (generated)
class PumpState(Record):
    running: bool
    fault: bool
    fault_code: int | None
    speed: float | None               # capability variable_speed
    pressure_out: float | None        # capability pressure
    hours: float | None               # capability hour_counter

class PumpTripped(Record):
    code: int

class Pump(ClassHandle[PumpState]):
    CLASS: ClassVar[str] = "std/pump@1"
    CAPABILITIES: ClassVar[tuple[str, ...]] = ("variable_speed", "pressure", "hour_counter")
    # members (layer 1 underneath): in, param and query are reads; out reads the applied value
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
    # methods: in the cycle, method record (no result); in async, await → results of the expanded writes
    def start(self) -> MethodTicket: ...
    def stop(self) -> MethodTicket: ...
    def run_at(self, pct: float) -> MethodTicket: ...        # pct in %
    # actions: async only
    async def reset(self, *, key: str | None = None, timeout: DurationLike | None = None) -> None: ...
```

```ts
// @joker/classes-std/pump.d.ts (generated)
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
// PumpTask<Req>: the same members; start/stop/run_at return Promise<WriteOutcome[]>;
// reset(o?: InvokeOpts): Promise<void>; tripped: EventMember<PumpTripped>
export declare const Pump: ClassSchema<PumpCycle<never>, PumpTask<never>, "variable_speed" | "pressure" | "hour_counter">;
```

The shared types of the classes:

```python
class Member(Generic[T]):                 # callable: p.running() ≡ get()
    def __call__(self) -> T: ...
    def get(self) -> T: ...
    def get_uncertain(self) -> T: ...
    def get_or(self, default: T) -> T: ...
    def sample(self) -> Sample[T]: ...
    async def history(self, start=None, end=None, *, last=None, step) -> History[T]: ...   # async only

class OutMember(Member[T]): ...           # reads the applied value (17 §11); written through methods or through raw()

class EventMember(Generic[R]):
    def events(self) -> AsyncIterator[R]: ...

class MethodTicket(Awaitable[list[WriteOutcome]]): ...   # not awaited in the cycle; in async, await

class HealthStatus(StrEnum):
    OK = "ok"; DEGRADED = "degraded"; FAILED = "failed"

@dataclass(frozen=True)
class Health:
    status: HealthStatus
    members: tuple[str, ...]              # the members that are not good

class ClassHandle(Generic[S]):
    name: str                             # name of the instance in the manifest
    index: int                            # position in a Many (0 outside a Many)
    def health(self) -> Health: ...
    def read(self) -> S: ...              # QualityError if Failed; capability members not good → None
    def raw(self) -> Any: ...             # layer 1 handles of the members (14 §3): p.raw().run_cmd.set(True)
    def has(self, capability: str) -> bool: ...

class Many(Generic[H]):
    def __len__(self) -> int: ...
    def __iter__(self) -> Iterator[H]: ...
    def __getitem__(self, i: int | str) -> H: ...   # position or name of the instance
    def get(self, name: str) -> H: ...           # unknown name → KeyError with J-E0807
    def at(self, i: int) -> H: ...               # out of range → IndexError with J-E0807
    def healthy(self) -> Iterator[H]: ...
    def read_all(self) -> list[Any]: ...         # QualityError if any is failed
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

### 14.2 Semantics

| Operation | Cycle | Async | Source |
|---|---|---|---|
| read member, query (`p.running()`, `p.failed_to_start()`) | latch | current slot | 14 §6.4; queries are derived signals of the core (14 A7) |
| method (`p.start()`) | method record in the buffer; `pending()` of the `out` stays `None` | `await` returns the `WriteOutcome`s of the expanded writes, after the commit | 17 §4.1 |
| action (`p.reset()`) | does not exist | request/response, key, timeout | 17 §4.2 |
| `health()` | rule of 17 §4.5 over the members of the latch | same, over the current slot | 17 §4.5 |
| `read()` | generated record; `QualityError` if `failed` | same | 14 §6.3 |
| `Many` | stable order (manifest, or name); size fixed at `init` | same | 14 §6.2 |
| `raw()` | layer 1 handles of the members | same | 14 §3 |

- **`health()` is computed by the SDK**, with the rule of document 17, §4.5: a pure function of the members' qualities, with no state and no time, which is not the case that A7 of document 14 forbids. To guarantee that Rust, TS and Python give the same result, the three SDKs pass the same test vectors, in `conformance/sdk/health.json`.
- **Capabilities.** In TS, the members of the capabilities listed in `requires` are required in the type; the others are optional (`Partial`), and a member of a capability the instance does not have is `undefined`. In Python, they are always in the type; accessing a member of a capability the instance does not have raises `CapabilityError` (`J-E0808`), and `p.has("pressure")` makes it possible to test. With `requires`, `jk check` guarantees them (`J0602`).
- **Units of method arguments:** the class's unit (`run_at(pct: f64 %)` receives `60.0` for 60%).
- **Name collisions.** The names of the handle API (`name`, `index`, `health`, `read`, `raw`, `has`) are reserved for class members (ref/00 §2, document 17 §3.2): `jkc` rejects them. A member with the name of a reserved word of the language (Python: `from`, `in`, `class`, …; TS: none, because properties accept reserved words) gets a `_` at the end in the generated code (`p.from_()`), and `jkc` warns.

### 14.3 Using classes

```python
from joker_classes.std import Led, Button

@app(name="hello_world", version="0.5.0")
class HelloWorld(App):
    led = device(Led)
    button = device(Button)

    @cycle("500ms")
    def control(self) -> None:
        if self.button.clicked():
            self.led.toggle()
```

The example of document 14, §6.6, uses `cycle("10ms")`, which is below the tier 2 minimum (§19, `J-E0854`). With 500 ms, a click shorter than the period is lost, because `clicked` = `rose(pressed)` is evaluated in the latch of whoever consumes it (14 A7). For buttons in Python or TS, the right form is a `task.on("<port>")` task on a port bound to the button's signal (every change is delivered), or an active implementation in Rust.

### 14.4 Local classes

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

It is bound with `each`/`map` (14 §6.5). A local class has no methods or contract queries: the behavior is app code, which in Python may be a method of the local class and in TS a function.

### 14.5 Implementing a class (virtual instance)

```python
from joker_classes.std import Pump

@implements(Pump, capabilities=["hour_counter"])
@app(name="pump_ctl", version="1.2.0")
class PumpCtl(App):
    cmd: Pump.Commands                    # annotation for the types; the SDK fills it in
    state: Pump.State
    contactor = Output(bool)
    feedback = Input(bool)
    thermal_trip = Input(bool)
    min_off = Param(float, unit="s", default=180.0)
    min_on = Param(float, unit="s", default=60.0)

    @on_action("reset")
    async def reset(self, args: None) -> None:
        ...                                # rearm; an exception becomes ActionError(device) for the caller
```

- `self.cmd` / `io.cmd` has the class's `out`s as inputs (received commands, already arbitrated and with envelope, 14 §7.2); `self.state` / `io.state` has the class's `in`s as published outputs; the `event`s are emitted with `self.state.<event>.emit(r)`.
- `@on_action("<action>")` / `onAction: { <action>: async (io, args, cx) => … }` receives the arguments already decoded and, optionally, a `cx` (`ActionCx`: `key`, `caller`, `deadline`, `progress(fraction, message)` over `report-progress`, `is_cancelled()`/`isCancelled()` over `is-cancelled`), and returns the result (or `None`). It maps to the `on-action` export (ref/04 §2.6.1). To end a canceled action, throw `ActionError.cancelled()` (`J-E0308`). An action of the class without a handler is `J-E0813` at build.
- `Pump.Commands` and `Pump.State` are generated by `jkc`. In TS, `io.cmd` and `io.state` have the right type without annotation.

### 14.6 Class members (nested handles)

A class member (document 17, §8) is a **property** of the handle with the handle of the member's class, as in Rust (ref/04 §4.8.6). User names stay in `snake_case` in both languages.

```ts
// generated from classes/local/bench_io.jkc (excerpt)
interface BenchIoCycleCore extends ClassHandleCycle<BenchIoState> {
  readonly status: LedCycle<never>;
  readonly boot: ButtonCycle<never>;
  readonly knob: AnalogInCycle<never>;
  rssi: Member<bigint>; uptime: Member<number>; online: Member<boolean>;
}
// a collection member: readonly pumps: Many<PumpCycle<never>>; an @optional: readonly grid?: SgReadyCycle<never>
```

```python
# generated from classes/local/bench_io.jkc (excerpt)
class BenchIo(ClassHandle[BenchIoState]):
    CLASS: ClassVar[str] = "local/bench_io@1"
    status: Led
    boot: Button
    knob: AnalogIn
    rssi: Member[int]
    uptime: Member[float]
    online: Member[bool]
# a collection member: pumps: Many[Pump]; an @optional: grid: SgReady | None
```

| Form | TypeScript | Python |
|---|---|---|
| query of a fixed member | `io.esp.boot.clicked()` | `self.esp.boot.clicked()` |
| collection | `for (const p of io.station.pumps) …` | `for p in self.station.pumps: …` |
| method of a member | `io.hp.compressor.stop()` | `self.hp.compressor.stop()` |
| `@optional` member | `io.hp.grid?.boost()` | `if self.hp.grid is not None: self.hp.grid.boost()` |

The semantics are those of §14.2 and of ref/04 §4.8.6: the writes belong to the app's writer, and in a virtual instance class members are read-only (the method is discarded with `J-E0507`, document 16, P6). The `health()` of the composite handle is that of the tree (17 §4.5), with the same test vectors of `conformance/sdk/health.json`.

---

## 15. Encodings in the guest

### 15.1 Image and proposals

The SDK decodes the packed image of `on-cycle` and writes the proposal buffer with the layout of ref/04 §3: 64 B header, 24 B samples and results, wide section; records `WRITE`, `METHOD`, `PUBLISH`, `COUNTER_ADD`, `EMIT` and `NOTE`. In Python, with `struct.unpack_from`/`pack_into` over a `memoryview`, with no copies per read; in TS, with `DataView` over the `Uint8Array`. Values of `string`, `bytes` and `Internal` up to 64 B go in the record itself, with the `INLINE` flag (ref/04 §3.3); `cycle-io.read` is only needed for the `ts_src`/`ts_hw` stamps (flags `HAS_TS_*` of the sample) and for dynamic access by name.

**Handles.** The handle of port *i* is its position in the port table, built in the order of the port list of `joker:meta` (ref/04 §2.6.2). The tier 2 SDKs emit the ports **ordered by name** (UTF-8 bytes), and not in declaration order: that way the order does not depend on details of the interpreter, and the same code gives the same `joker:meta` byte for byte. Rust uses the order of the struct's fields; the host does not distinguish.

### 15.2 Records

- Deterministic CBOR (RFC 8949, §4.2.1), with the shape of D2 (ref/00 §4.1, ref/04 §4.10): a map with **integer keys**, the position of the field in the `joker:meta` schema counting from 0; integers in the shortest form; `f64` in the shortest form that preserves the value; an optional field set to `None`/`null` is omitted. `Plan(target_soc=0.62, limit_kw=400.0, valid_until=…)` is `{0: 0.62, 1: 400.0, 2: …}`.
- Each SDK has its own encoder (without dependencies), and all of them pass the vectors of `conformance/sdk/cbor.json`: the same records give the same bytes in Rust, TS and Python.
- Limits (ref/00 §10): `string` ≤ 1 KiB, lists ≤ 64, `Internal` ≤ 64 B encoded. Outside that, `LimitError` on encoding (`J-E0805`, `J-E0804`).
- The text interfaces (API, `jk`, SDK logs) show the fields by name, from the schema.

---

## 16. Mapping to the `joker:runtime` world

Each function of the world (`wit/joker-runtime.wit`, ref/04 §2) has its form in the three languages. "Image" and "buffer" mean that the SDK does not call the host: it uses the image of `on-cycle` or writes a record in the proposal buffer (ref/04 §3). The `@unstable` items (1.1) exist in the tier 2 types, with the behavior of the Rust SDK (§7, ref/04 §2.9): rejected in `prod` (`J-E0003`), and in `dev` they raise `UnavailableError` with the stubs' `J-E0601`.

**`cycle-io` (cycle instance)**

| WIT | Rust (ref/04) | Python | TypeScript | Path |
|---|---|---|---|---|
| `read` | `get`, `get_uncertain`, `get_or`, `sample` | `get()`, `get_uncertain()`, `get_or()`, `sample()` | `get()`, `getUncertain()`, `getOr()`, `sample()`, `tryGet()` | image; `read` for `ts_src`/`ts_hw` |
| `write` | `CycleOutput::set` | `OutputHandle.set` | `OutputCycle.set` | buffer (`WRITE`) |
| `relinquish` | `relinquish` | `relinquish()` | `relinquish()` | buffer (`WRITE` with `RELINQUISH`) |
| `pending` | `pending` | `pending()` | `pending()` | SDK (local buffer) |
| `last-outcome` | `last_outcome`, `mine`, `winner` | `last_outcome()` | `lastOutcome()` | image (results section) |
| `param` | `ParamHandle::get` | `ParamHandle.get()` | `ParamHandle.get()` | image |
| `call` | class method (`p.start()`) | `p.start()` | `p.start()` | buffer (`METHOD`) |
| `publish` | `PublishedHandle::set`, `AlarmHandle::set/raise/clear`, `CycleInternal::set` (cycle writer), `io.state.<m>.set` | `Published.set`, `Alarm.set/raise_/clear`, `Internal.set`, `self.state.<m>.set` | `set`, `raise`/`clear`, `set`, `io.state.<m>.set` | buffer (`PUBLISH`) |
| `counter-add` | `CounterHandle::inc/add` | `Counter.add(n)` | `CounterHandle.add(n)` | buffer (`COUNTER_ADD`) |
| `emit` | `EventOutputHandle::emit`, `io.events.<e>.emit` | `EventOutput.emit`, `self.state.<e>.emit` | `emit` | buffer (`EMIT`) |
| `note-substituted` | `get_or` | `get_or()` | `getOr()` | buffer (`NOTE`) |
| `mailbox-send` | `MailboxEnd::try_send` | `Mailbox.try_send` | `MailboxSend.trySend` | host |
| `mailbox-recv` | `MailboxEnd::try_recv` | `Mailbox.try_recv`, `drain` | `MailboxRecvCycle.tryRecv`, `drain` | host |
| `now` | `CycleIo::now` | `self.now()` (and `time.*`) | `io.now()` (and `Date.now()`) | image (header); `wasi:clocks` served with the latch |
| `dt-ns` | `CycleIo::dt` | `self.dt()` | `io.dt()` | image (header) |
| `random-u64` | `CycleIo::random` | `random` (seeded) | `Math.random()` (seeded) | host, via `wasi:random` |
| `write-at` (1.1) | `write_at` (`unstable-scheduled-writes`) | `set_at` | `setAt` | buffer (`WRITE` with `AT`) |

**`async-io` (async instance)**

| WIT | Rust (ref/04) | Python | TypeScript |
|---|---|---|---|
| `read` | `TaskInput::get`, `sample`, … | `get()`, `sample()`, … | `get()`, `sample()`, … |
| `snapshot` | `TaskIo::snapshot` | `self.snapshot(*ports)` | `io.snapshot([...])` |
| `set` | `TaskOutput::set` | `OutputHandle.set` | `OutputTask.set` |
| `write` | `TaskOutput::write(v).await` | `await write(v)` | `await write(v)` |
| `relinquish` | `relinquish` | `relinquish()` | `relinquish()` |
| `last-outcome` | `last_outcome` | `last_outcome()` | `lastOutcome()` |
| `param` | `ParamHandle::get` | `ParamHandle.get()` | `ParamHandle.get()` |
| `call` | class method `.await` → `MethodOutcome` | `await p.start()` → `list[WriteOutcome]` | `await p.start()` → `WriteOutcome[]` |
| `invoke` | `ActionPort::invoke(args).await`, `p.reset().await` | `await invoke(args, key=)` | `await invoke(args, { key })` |
| `start-action`, `action-call.*` | `ActionCall::start`, `ActionHandle::{progress, cancel, finish}` | `invoke_long()`, `LongAction.{progress, cancel, finish}` | `invokeLong()`, `LongAction.{progress, cancel, finish}` |
| `publish` | `PublishedHandle::set`, `AlarmHandle::set`, `TaskInternal::set` (async writer) | `set`, `Alarm.set`, `Internal.set` | `set`, `set`, `set` |
| `counter-add` | `CounterHandle::add` | `Counter.add` | `CounterHandle.add` |
| `emit` | `EventOutputHandle::emit` | `emit` | `emit` |
| `note-substituted` | `get_or` | `get_or()` | `getOr()` |
| `subscribe` | `TaskInput::subscribe` | `self.subscribe(*ports)` | `io.subscribe([...])` |
| `events` | `EventInputHandle::next/stream` | `EventInputHandle.events()` | `EventInputHandle.events()` |
| `mailbox-send`, `mailbox-recv` | `MailboxEnd::try_send/try_recv` | `try_send`, `try_recv`, `drain` | `trySend`, `tryRecv`, `drain` |
| `mailbox-next` | `MailboxEnd::recv().await` | `await recv()` | `await recv()` |
| `history` | `history`, `history_agg` | `await history(…, step=, agg=)` | `await history({ …, step, agg })` |
| `now` | `TaskIo::now` | `self.now()` | `io.now()` |
| `sleep-until` | `TaskIo::sleep`, `sleep_until` | `await self.sleep()`, `sleep_until()` (and `asyncio.sleep`) | `await io.sleep()`, `sleepUntil()` (and `setTimeout`) |
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
| `log.log` | `log::info!(…)` and the other macros | `log.info(…)`, `logging`, `print` | `log.info(…)`, `console.*` |
| `log.enabled` | `joker::log::enabled(Level)` | `log.enabled(level)` | `log.enabled(level)` |

**`guest` (exports)**

| WIT | Rust (ref/04) | Python | TypeScript |
|---|---|---|---|
| `init` | generated by `#[joker::app]` | SDK (stores parameters, seeds `random`, resolves handles) | same |
| `on-params` | `#[joker::on_params]` | `@on_params` | `onParams` |
| `on-cycle` | `#[joker::cycle]` | `@cycle` | `cycles: { … }` |
| `run-task` | `#[joker::task]` | `@task.*` | `tasks: { … }` |
| `on-action` | `#[joker::on_action]` | `@on_action` | `onAction` |
| `export-state`, `migrate` | `#[joker::state(persist)]`, `#[joker::migrate]` | `@state(persist=True)`, `@migrate` | `cycle({ persist: true })`, `migrate` |

**Reserved (1.1):** `ml.infer` and the `tensor` resource (`dtype`, `shape`, `data`) → `Model.infer`, `Tensor`; `frames.next` and the `frame` resource (`meta`, `view`) → `StreamInput`; `blocks.next` → `BlockInput`: in the types; in v1, `J-E0003` in `prod` and `UnavailableError` (`J-E0601`) in `dev` (the Rust names are those of ref/04 §4.15). **WASI:** `wasi:clocks` (latch in the cycle, real in async), `wasi:random` (the deterministic generator), `wasi:http/outgoing-handler` (`fetch`, `joker_sdk.http`, with `net.allow`), as in ref/04 §2.7.

## 17. Tests

The harness is the `joker-test` crate of the Rust SDK (04 §3.7), wrapped: **PyO3** in the `joker-testing` package and **napi-rs** in `@joker/test`, with prebuilt wheels and binaries for Linux (x86_64, aarch64), macOS (aarch64, x86_64) and Windows (x86_64). It runs the real component (`.wasm`) in wasmtime, with the virtual clock and the real `joker-core`: the same harness, the same semantics, in the three languages.

### 17.1 API

```python
class Harness(Generic[AppT]):
    @classmethod
    def for_app(cls, app: type[AppT] | None = None, *, wasm: str | os.PathLike[str] | None = None) -> Harness[AppT]: ...
    # configuration (chainable)
    def group(self, task: str, period: DurationLike) -> Self: ...
    def param(self, name: str, value: Any) -> Self: ...
    def input(self, name: str, value: Series | Any) -> Self: ...
    def internal(self, name: str, value: Any) -> Self: ...
    def history(self, name: str, series: Series, *, before: DurationLike) -> Self: ...   # preloaded historian
    def instance(self, port: str, name: str, *, capabilities: Sequence[str] = (), sim: bool = True) -> Self: ...
    def member(self, instance: str, member: str, value: Series | Any) -> Self: ...
    def action(self, target: str, handler: Callable[[Any], Any] | BaseException) -> Self: ...
    def envelope(self, output: str, *, range: tuple[Any, Any] | None = None, rate: str | None = None,
                 safe: Any = None, interlock: str | None = None) -> Self: ...
    def clock(self, *, start: str = "2026-01-01T00:00:00Z", quality: str = "synced") -> Self: ...
    def seed(self, seed: int) -> Self: ...
    # execution, in virtual time
    def run_for(self, d: DurationLike) -> None: ...
    def step(self, ticks: int = 1) -> None: ...
    def run_task(self, task: str) -> None: ...          # forces one execution of an async task
    def now(self) -> Instant: ...
    # observation
    def output(self, name: str) -> OutputTrace: ...     # proposals, results and applied values
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

- **Series:** `Series.constant(v)`, `Series.step(before, after, at)`, `Series.ramp(a, b, over)` (stays at `b` after `over`), `Series.linear(a, b, over)` (continues the line), `Series.sine(mean, amp, period)`, `Series.noise(mean, std, seed=)`, `Series.replay(path, column)` (Parquet), and `.with_quality_at(t, q)`/`.withQualityAt(t, q)`, `.with_quality(start, end, q)`. The same as in `joker-test`.
- **Traces:** `.values()`, `.last()`, `.when(cond)`, `.after(t)`, `.before(t)`, `.all(pred)`, `.any(pred)`, `.no_new_writes()`/`.noNewWrites()`, `.outcomes()`, `.lt(x)`/`.gt(x)`/`.eq(x)` (for conditions).
- **Envelope:** without `envelope()`, every proposal is `accepted`. With `envelope()`, the harness uses the real envelope of `joker-core` with that configuration, and the results are those of production.
- **Class instances:** `instance(port, name, sim=True)` binds an instance to the class simulator (14 A13); with `sim=False`, the members come from `member()`.
- **Async tasks** run on the same virtual clock; `history()` is served from what the harness has already generated plus the preloaded data.

### 17.2 pytest

```python
# tests/test_peak_shaver.py
from joker_testing import Harness, Series, Quality
from peak_shaver import PeakShaver, Plan

def test_never_discharges_below_the_reserve() -> None:
    h = (Harness.for_app(PeakShaver)
         .group("control", "200ms")
         .param("limit", 400.0).param("reserve", 10.0)
         .input("grid_power", Series.ramp(300.0, 700.0, "10m"))
         .input("soc", Series.linear(0.30, 0.05, "10m"))
         .internal("plan", Plan(target_soc=0.5, limit_kw=400.0, valid_until=2**62)))
    h.run_for("10m")                                    # virtual clock: runs in milliseconds
    soc = h.input_series("soc")
    assert h.output("battery").when(soc.lt(0.1)).all(lambda v: v == 0.0)
    assert h.traps() == []

def test_bad_quality_does_not_write() -> None:
    h = (Harness.for_app(PeakShaver)
         .input("grid_power", Series.constant(500.0).with_quality_at("30s", Quality.bad("comm_failure")))
         .input("soc", 0.5))
    h.run_for("60s")
    assert h.output("battery").after("30s").no_new_writes()
```

The plugin (`pytest11`, activated when `joker-testing` is installed) runs `jk build` on the app once per session, only if the sources changed, and `Harness.for_app` uses that `.wasm`. `jk test` runs `pytest` for Python apps.

### 17.3 vitest and `node:test`

```ts
// test/peak_shaver.test.ts
import { describe, expect, it } from "vitest";
import { Harness, Series, Quality } from "@joker/test";
import peakShaver from "../src/app.js";

describe("peak_shaver", () => {
  it("never discharges below the reserve", () => {
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

`vitest.config.ts` with `globalSetup: ["@joker/test/setup"]` does the build once. With `node:test`, `import "@joker/test/register"` does the same at the first `Harness.forApp`. The API does not depend on the *framework*: it also works with Jest.

### 17.4 Native mode

`import joker_sdk` in a normal CPython, and `import "@joker/sdk"` in a normal Node, work in **native mode**: the declarations and the pure functions of the app can be tested directly (`Plan.from_peak(…)`), without a harness. Any host call raises `J-E0818`. It is also the mode `jk build` uses to extract the metadata (§18.4).

---

## 18. Packaging and build

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

### 18.3 Keys

The `[tool.joker]` and `"joker"` keys are the same. They are files written by people, and therefore strict: an unknown key is `J0140`; the `x_*` keys are ignored (ref/00 §1).

| Key | Type | Required | Default | Validation |
|---|---|---|---|---|
| `app` | Python: `"module:Class"`; TS: path of the entry file | yes | — | must have exactly one app (`J-E0850`) |
| `world` | `"joker:runtime@X.Y.Z"` | no | the SDK's world | must be a world the installed SDK knows how to generate (`J-E0856`) |
| `include` | list of globs relative to the package | no | `[]` | embedded read-only data files; total ≤ 4 MiB (`J-E0860`) |

**There are no more keys, on purpose.** The name, the version, the ports, the tasks, the units and the alarms are in the code (04 §1). The memory, the group, the level in the arbiter and the network belong to the site (`[apps.<a>]`, 03 §8). The `version` in the code is optional: without it, the one in `[project]`/`"version"` applies; with both, they must be equal (`J-E0852`). The `name` in the code is required (it is the app's name in the manifest, in `snake_case`), and the package's name is free (`peak-shaver`).

### 18.4 What `jk build` does

1. **Detects the language:** `Cargo.toml` → Rust (ref/04); `package.json` with `"joker"` → TS; `pyproject.toml` with `[tool.joker]` → Python. More than one in the same directory is `J-E0850`.
2. **Checks the toolchain:** TS requires Node ≥ 20 and `@joker/sdk` in `node_modules`; Python requires the interpreter of `requires-python` and `joker-sdk` in the active environment (or in `.venv`), which pins the version of `componentize-py`. If missing: `J-E0861`, with the command that fixes it.
3. **Runs the declaration in native mode** (§17.4): imports the entry point, collects the registry, validates (§5.5), produces `joker:meta`, and runs the static *lints*: `J-E0859` (async API in a cycle task), `J-E0858` (module global written in a task) and `J-E0863` (generic exception catch around reads in a cycle task).
4. **Packages:** TS with esbuild (ESM, `es2022`, a single file, with `@joker/sdk` included; `node:*` imports are `J-E0853`); Python with the entry package and the dependencies of the environment, which must be pure Python or have `wasm32-wasi` wheels (`J-E0853`, with the list of those that fail).
5. **Componentizes with pre-initialization:** ComponentizeJS (StarlingMonkey, with weval) or componentize-py (CPython, with Wizer), against the `wit/` of the requested world. The top-level code runs again inside the Wasm, and the SDK compares the digest of the registry with that of step 3: a difference is `J-E0862` (the declaration depends on the environment).
6. **Injects** the `joker:meta` section and writes `dist/<app>.wasm`. With `--target`, it precompiles the `.cwasm` (01 §9).
7. **Checks the size limits** of §19 (`J-E0860`).

The build is reproducible: the same sources, the same dependencies (pinned by the ecosystem's lock: `package-lock.json`, `uv.lock` or `pylock.toml`), the same version of the SDK and of the toolchain give the same bytes. Without an ecosystem lock, `jk build` warns (`J-E0864`), because the digest of the Joker lock stops being reproducible from Git.

`jk app new <name> --lang ts|py` generates the `package.json` or the `pyproject.toml`, the entry point, a test, the ecosystem lock and, in TS, the `tsconfig.json` (`strict`, `noUncheckedIndexedAccess`) and the lint rule `no-floating-promises`.

---

## 19. Tier 2 limits

**Tier 2 runs in the cycle and in async**, with a minimum cycle period. Forbidding the cycle would force slow controls that TS and Python do well (HVAC at 1 s, tank levels at 1 s) to be written in Rust, and would push people towards `every("1s")` tasks in async, which have no latch, no determinism and no real-time admission. What makes the cycle acceptable is that GC pauses happen **inside** the tick, are measured by `jk bench` and enter the admission (M23).

| | Rust (04 §4.3) | TypeScript | Python | Where it applies |
|---|---|---|---|---|
| minimum period of a cycle task | 1 ms | **100 ms** | **500 ms** | `jk check` and `jk build`: `J-E0854` (error in `prod`, warning in `dev`) |
| default `wcet` | declared | 20% of the period | 20% of the period | until there is a measurement in the lock |
| maximum `wcet` | admission | 50% of the period | 50% of the period | `J-E0854` |
| admission with measurement | p99.9 | measured **maximum** | measured **maximum** | `jk bench` runs ≥ 10,000 ticks and records `p999` and `max` in the lock's `[wcet]`; tier 2 admission uses `max` (D15, ref/02 §2.1.13) |
| epoch deadline | `max(2 × wcet, 200 µs)` (01 §4) | same | same | host |
| default `memory.max` (cycle / async) | 16 / 64 MiB (01 §9) | **64 / 64 MiB** | **128 / 128 MiB** | the SDK puts the suggestion in `joker:meta`; the manifest's `[apps.<a>] resources.memory` wins; it enters `J0380` |
| component size (`.wasm`) | 0.1–2 MB | target 8–12 MB, maximum 32 MB | target 15–30 MB, maximum 64 MB | `J-E0860` |
| cold start (from the cached `.cwasm` to the end of `init`), p95 on a CM4 | < 10 ms | **≤ 100 ms** | **≤ 300 ms** | target; pre-initialization and the *copy-on-write* memories of pooling make it possible |
| AOT on the device (without a precompiled `.cwasm`), CM4 | seconds | ≤ 60 s | ≤ 180 s | target; that is why `jk build --target` is the normal path |
| `on-cycle` overhead with an empty task and 20 ports | ≤ 5 µs | ≤ 200 µs | ≤ 1 ms | target |
| GC pause p99.9 with 32 MiB of live heap | — | ≤ 5 ms | ≤ 20 ms | target; measured in `jk bench` |
| hot swap | 0 ticks lost | 0 ticks lost | 0 ticks lost | the new instance is created alongside (01 §9) |
| types and units | compilation | types at compilation (`tsc`); units in `jk build` | types in mypy/pyright; units in `jk build` | `J0109`, `J0107` in `jk check` as in Rust |
| architectures | all (01 §22) | x86_64, aarch64, riscv64 (Cranelift) | same | armv7 (Pulley) is `J-E0855`: a JS or Python interpreter on top of a Wasm interpreter is 50–100× slower |
| Joker Micro | yes (native) | **no** | **no** | `jk build --target mcu:…` fails with `J0720` |

- The "target" figures are estimates to validate when each SDK exists (TS in Phase 3, Python in 1.2), like those of document 01, §21. The period minimums, the WCET and size maximums, and the architectures are **rules**, applied by `jk check`.
- A tier 2 task may share a group with Rust tasks, if the group's period respects the minimum. The GC pause delays the following tasks of the same tick, and the admission counts it. For fast loops, the rule of document 04 still holds: Rust.
- If the Component Model *async* (WASI 0.3) is not stable in ComponentizeJS or in componentize-py when the SDK ships, **plan B** of §29, D9 applies: the `app-blocking` world of ref/04 §2.11.

---

## 20. Parity: the same app in three languages

The two apps below are the proof that the API closes. The Rust one follows document 04 and is **illustrative** (the normative syntax is that of ref/04); the TS and the Python ones are normative. With one deliberate exception in §20.1 (the period and the WCET, because of §19), the three versions have the same ports, the same names, the same write results and the same quality semantics.

### 20.1 `peak_shaver` (document 04, §3)

Version of v1.0, without ML: the plan is computed from the historian. The model port and the `infer` of 1.1 are at the end of the section. Two corrections with respect to document 04 (§33, A3 and A4): the port is called `grid_power`, as in the `bind` of document 03, §8, and, with the plan expired or not yet existing, the limit is that of the parameter (with the `Plan::default()` of document 04, the limit would be 0 kW and the battery would discharge to cover all the import from the grid).

**Rust** (illustrative):

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
    #[alarm(severity = "medium", message = "plan expired")] plan_stale: Alarm,
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
  alarms: { plan_stale: alarm({ severity: "medium", message: "plan expired" }) },

  cycles: {
    control: cycle({ period: "200ms", wcet: "20ms", state: () => ({ integral: 0 }) }, (io, st) => {
      const grid = io.grid_power.get();          // QualityError → this task's tick ends here
      const soc = io.soc.get();
      const plan = io.plan.getOrDefault();
      const stale = plan.valid_until < io.now().utcSeconds();
      io.plan_stale.set(stale);
      const limit = stale ? io.limit.get() : Math.min(plan.limit_kw, io.limit.get());
      const excess = grid - limit;
      st.integral = Math.min(Math.max(st.integral + excess * io.dt().seconds, -50), 50);
      const floor = io.reserve.get() / 100;      // % → fraction, like soc
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
    plan_stale = Alarm(severity="medium", message="plan expired")
    low_soc = EventOutput(LowSoc)

    @cycle("500ms", wcet="100ms", state=Ctl)
    def control(self, st: Ctl) -> None:
        grid = self.grid_power.get()            # QualityError → this task's tick ends here
        soc = self.soc.get()
        plan = self.plan.get_or_default()
        stale = plan.valid_until < self.now().utc_seconds()
        self.plan_stale.set(stale)
        limit = self.limit.get() if stale else min(plan.limit_kw, self.limit.get())
        excess = grid - limit
        st.integral = min(max(st.integral + excess * self.dt().seconds, -50.0), 50.0)
        floor = self.reserve.get() / 100.0      # % → fraction, like soc
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

**The deliberate difference:** the Python `control` asks for `"500ms"`, because 200 ms is below the Python minimum (§19). On a site with the `fast` group at 200 ms, the Python version does not bind to that group (`J-E0854`), and it is `jk check` that says so. The integral control uses `dt()`, and so the logic does not change with the period (04 §3.2).

**Correspondence:**

| Concept | Rust | TypeScript | Python |
|---|---|---|---|
| read with quality | `io.soc.get()?` | `io.soc.get()` (throws) | `self.soc.get()` (raises) |
| internal with default | `io.plan.get_or_default()` | `io.plan.getOrDefault()` | `self.plan.get_or_default()` |
| cycle state | `st: &mut Ctl` | `st` from the `state` factory | `st: Ctl` |
| effective period | `io.dt().as_secs_f64()` | `io.dt().seconds` | `self.dt().seconds` |
| proposal | `io.battery.set(sp)` | `io.battery.set(sp)` | `self.battery.set(sp)` |
| history | `.history(now - 24.h(), now, 15.mins()).await?` | `await .history({ start: now.minus("24h"), end: now, step: "15m" })` | `await .history(now - "24h", now, step="15m")` |
| only the good ones | `hist.iter().filter_map(\|s\| s.good().copied())` | `hist.values()` | `hist.values()` |
| trigger on change | `#[joker::task(on = "soc")]` | `task.on("soc", …)` | `@task.on("soc")` |
| sample of the trigger | `s.get()` | `s.isGood()`, `s.value` | `s.is_good()`, `s.value` |

**The same app with the model, in 1.1** (reserved; in v1 the `Model` port is not bound, the component is rejected in `prod` with `J-E0003`, and in `dev` `infer` raises `UnavailableError` with `J-E0601`):

```python
    forecast = Model(inputs="f32[1,96,4]", outputs="f32[1,96]")
    # in replan: y = await self.forecast.infer([x]); y.quality, y.outputs[0]
```

```ts
  models: { forecast: model({ inputs: "f32[1,96,4]", outputs: "f32[1,96]" }) },
  // in replan: const y = await io.forecast.infer([x]);
```

### 20.2 `pump_duty`: classes, `Many<Pump>`, methods, queries, actions

A business app over `std/pump@1` (17 §3.1): it starts the lead pump when the level drops, chooses the lead **at the start of each demand** (the healthy one with the fewest hours), moves on to the next one when the lead fails to start, and requests the rearm in async. It is a reduced version of §9.1 of document 14, with the fix of finding A6 (§33): there, the lead is chosen again in every tick, and two pumps with close hours swap the lead while the pump is running.

**Rust** (illustrative):

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
    for p in io.pumps.iter() {                         // the methods of the handles are &self
        let i = p.index();
        if st.demand && i == st.lead { p.start() } else { p.stop() }
        let failed = p.failed_to_start()?;
        if failed && !st.failed[i] {
            st.n += 1;
            let pump = BoundedString::try_from(p.name()).unwrap_or_default();
            let _ = io.failures.try_send(PumpFailed { pump, n: st.n });   // full: counted drop
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
            Ok(()) => log::info!("rearm requested"; pump = f.pump.as_str()),
            Err(e) => log::warn!("rearm failed"; pump = f.pump.as_str(), code = e.error.code().as_str()),
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
          log.info("rearm requested", { pump: f.pump });
        } catch (e) {
          if (!(e instanceof ActionError)) throw e;
          log.warn("rearm failed", { pump: f.pump, code: e.code });
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
                log.info("rearm requested", pump=f.pump)
            except ActionError as e:
                log.warn("rearm failed", pump=f.pump, code=e.code)
```

- A `QualityError` in `p.failed_to_start()` ends the tick **after** the proposals of the previous pumps, which remain; the following ones keep the lease of the previous tick until it expires (16 A2). It is the semantics of Rust's `?`.
- `p.start()` in the cycle is a method record (17 §4.1); `jk check` knows that it writes `run_cmd`. `p.reset()` only exists in async, with an idempotency key derived from the failure number: a retry for the same failure does not repeat the rearm (16 E9).
- The rearm policy belongs to the site: the action's `permit` and `role` come from the manifest (17 §4.2), and a `denied` arrives as `ActionError`.

---

# Part B — Local HTTP API client libraries

## 21. Packages and generation

| Library | Registry | Import | Dependencies | Who uses it |
|---|---|---|---|---|
| `joker-client` (Rust) | crates.io | `joker_client` | `tokio`, `hyper` 1 + `hyper-util`, `rustls`, `tokio-tungstenite`, `serde` | `jk`, the Deck (via tunnel, 1.3), tools in Rust |
| `joker-client` (Python) | PyPI | `joker_client` | `httpx` ≥ 0.27, `websockets` ≥ 13, `pydantic` ≥ 2; extra `[pandas]`: `pandas` ≥ 2.2, `pyarrow` ≥ 15 | notebooks, data scripts, integration |
| `@joker/client` | npm | `@joker/client` | `undici` ≥ 6, `ws` ≥ 8; Node ≥ 20 | commissioning scripts, integrations, the Studio (1.3) |

**Generated types, hand-written API.** The three libraries have two layers:

1. **Generated layer** from `spec/openapi/joker-api.yaml` (ref/09), with the command `cargo xtask gen-clients`:
   - the **schemas** (Rust with `typify`; Python with `datamodel-code-generator` for pydantic v2 models with `extra = "ignore"`; TS with `openapi-typescript`);
   - an **operation table** by `operationId`: method, path, parameters, minimum role, whether it is mutating and whether it is idempotent.
   The generated files live in the repository, and CI runs `cargo xtask gen-clients --check`: a difference between the OpenAPI and the clients fails the build.
2. **Hand-written layer** on top: connection (Unix socket, mTLS), contexts, anti-replay (`nonce`, `ts`), retries, `problem+json` errors as exceptions with a code, pagination, WebSocket subscriptions, unit and time conversions, and the export to pandas.

Why not everything generated: OpenAPI generators do not handle Unix sockets, mTLS with contexts, WebSocket, anti-replay or *streaming*, and the result is not idiomatic in any of the three languages. Why not everything by hand: the schemas would drift from ref/09 without anyone noticing. The generated operation table is the meeting point: a method of the hand-written layer only calls an `operationId` that exists.

**Versions.** The client's major is the API's (`/v1` → clients 1.x). A 1.y client talks to any 1.x node: unknown fields are ignored and unknown enum values arrive as `unknown` (ref/00 §1, rule 2). On `connect`, the client reads `getInfo` (API version and the `features` list, ref/09 §5.1) and, if needed, `getVersion`; calling an operation the node does not have (by the OpenAPI's `x-joker-since` or by a missing `feature`) gives `J-E0883` with the node's version, before sending the request.

---

## 22. Connection and nodes

### 22.1 Addresses

The addresses are those of `--node` (ref/02 §3.3.1, and `nodes.<name>.address` of §2.3.2): `local`, a context name, `unix:<path>`, `<host>:<port>` and `ssh://[<user>@]<host>[:<port>]`, and only these (ref/09 §2.1, document 07 §1):

| Form | Transport | Authentication | Role |
|---|---|---|---|
| `unix:/run/joker/api.sock` | HTTP/1.1 over Unix socket | process credentials (`SO_PEERCRED`) | `local:uid=…`, `local:group=…` (ref/02 §3.3.2, 03 §13) |
| `<host>:<port>` (port 7443 by default) | HTTP/2 over TLS 1.3 | **mTLS** always (01 §14); the node's certificate verifies against the CA **and** has the node's name in the SAN (`J0912`) | by the certificate's CN (`cert:CN=ops-*`) |
| `ssh://[<user>@]<host>[:<port>]` | SSH tunnel to the node's Unix socket | that of SSH | that of the SSH user on the node (ref/02 §3.3.2) |
| `nodes.<n>.via = "deck:<name>"` | remote session through the Deck | that of the Deck | **1.3**, reserved (ref/02) |

- Rust implements `ssh://` with the same crate as `jk`. Python and TS use the system's `ssh` with socket forwarding (`ssh -N -L <temporary socket>:/run/joker/api.sock`), which requires OpenSSH ≥ 6.7; without `ssh` in the `PATH`, `J-E0881` with the hint.
- There is no insecure mode and no trust on first use (ref/02 §3.3.1).

### 22.2 Resolution

The libraries resolve the node **by the rules of `jk`** (ref/02 §3.3.1 and §2.3.3), so that a script and `jk` talk to the same node without repeated configuration:

1. an explicit address (`Client.connect("lisboa-01.example.com:7443", identity=…)`), without reading configuration;
2. otherwise, the value of `node` given to `Client.from_config(node)`, or `JK_NODE`, or `defaults.node` (with the precedence of ref/02: argument > variable > the project's `.joker/config.toml` > the user's `config.toml`), resolved by the 5 steps of ref/02 §3.3.1 (`local`, literal address, name in `[nodes]`, host of a single context, `<value>:7443`);
3. with none of them, `local` (the project's `jk dev`, or `/run/joker/api.sock`).

The identity is that of `[user]` or that of the context (`cert`, `key`, `ca`, including `pkcs11:`), or `JK_CERT`/`JK_KEY`/`JK_CA`. The libraries **read** the configuration (`JK_CONFIG` included) and never write it. The errors are those of ref/02: `J0910` (invalid configuration), `J0911` (unknown or ambiguous node), `J0912` (node certificate), `J0913` (key with insecure permissions). The library exposes the origin of the choice (`client.origin`, for example `"lisboa-01 · from JK_NODE"`), so that a script can show the same line that `jk` shows before changing the node (ref/02 §3.3.1).

### 22.3 Construction

```python
from joker_client import AsyncClient, Client, Identity

jk = Client.from_config()                                    # rules of §22.2
jk = Client.from_config("lisboa-01", timeout="30s")
jk = Client.connect("unix:/run/joker/api.sock")
jk = Client.connect("lisboa-01.example.com:7443",
                    identity=Identity(cert="ops.crt", key="ops.key", ca="site-ca.crt"))
async with AsyncClient.from_config("lisboa-01") as ajk:     # the same API, with await
    ...
```

```ts
import { Client } from "@joker/client";
const jk = await Client.fromConfig();                        // rules of §22.2
const jk2 = await Client.connect("lisboa-01.example.com:7443", {
  identity: { cert: "ops.crt", key: "ops.key", ca: "site-ca.crt" },
});
```

```rust
let jk = joker_client::Client::from_config(None).await?;
let jk = joker_client::Client::connect(&"unix:/run/joker/api.sock".parse()?, None).await?;
```

Python has a **synchronous** client (`Client`, for notebooks and scripts) and an **asynchronous** one (`AsyncClient`), with the same methods. TS and Rust are asynchronous only.

---

## 23. Surface

Each method corresponds to an `operationId` of the OpenAPI (`spec/openapi/joker-api.yaml`), which is that of ref/09. The `jk` column says which CLI command uses the operation, because `jk` is only a client (01 §14, 20 §2).

| Operation | `jk` | `operationId` | Python | TypeScript | Role | Version |
|---|---|---|---|---|---|---|
| node information | `jk status` | `getInfo`, `getVersion` | `info()`, `version()` | `info()`, `version()` | viewer | 1.0 |
| status | `jk status [--instances]` | `getStatus` | `status()` | `status()` | viewer | 1.0 |
| read signals | `jk read <sel>` | `readValues` (`queryValues` with many selectors) | `read(sel, *, coherent=False, mono=False)` | `read(sel, o?)` | viewer | 1.0 |
| force a read | `jk read --refresh` | `refreshValues` | `read(sel, refresh=True)` | `read(sel, { refresh: true })` | operator | 1.0 |
| subscribe | `jk read --watch`, `jk logs --follow` | `websocket` (and `sse`) | `subscribe(...)` | `subscribe(...)` | viewer | 1.0 |
| history | `jk history` | `queryHistory` (`getHistory`) | `history(sel, start, end, *, step=None, agg=None)` | `history(sel, o)` | viewer | 1.0 |
| export Parquet | `jk export --parquet` | `queryHistory` in NDJSON | `export(sel, start, end, path)` (the Parquet is written by the library) | `export(sel, o)` | viewer | 1.0 |
| preview a write | `jk write` (before confirming) | `previewManualWrite` | `write(…, dry_run=True)` | `write(…, { dryRun: true })` | viewer | 1.0 |
| manual write | `jk write` | `manualWrite` | `write(path, value, *, ttl, reason, level=None, takeover=False)` | `write(path, value, o)` | operator | 1.0 |
| release a level | `jk release` | `releaseManual` | `release(path, *, level=None, reason)` | `release(path, o)` | operator | 1.0 |
| preview a method | `jk call` (before confirming) | `previewMethodCall` | `call(…, dry_run=True)` | `call(…, { dryRun: true })` | viewer | 1.0 |
| call a method | `jk call` | `callMethod` | `call(target, args=None, *, ttl, reason)` | `call(target, args, o)` | operator | 1.0 |
| invoke an action | `jk action` | `invokeAction`, `getInvocation`, `cancelInvocation` | `invoke(target, args=None, *, key=None, timeout=None)` | `invoke(target, args, o?)` | the one in the manifest (min. operator) | 1.0 |
| alarms | `jk alarms`, `jk alarm ack/shelve` | `listAlarms`, `getAlarm`, `ackAlarm`, `shelveAlarm`, `unshelveAlarm`, `listAlarmEvents` | `alarms.list()`, `.ack(id, *, reason=None)`, `.shelve(id, ttl, reason)`, `.unshelve(id)`, `.events(…)` | same, camelCase | viewer / operator | 1.0 |
| trips | `jk trips`, `jk reset` | `listTrips`, `resetTrip`, `resetTrips` | `trips.list()`, `trips.reset(path, *, reason)` | same | viewer / operator | 1.0 |
| decisions | `jk decisions` | `listDecisions` | `decisions.query(*, output=None, cause=None, start=None, end=None, since=None)` | `decisions.query(o)` | viewer | 1.0 |
| generations | `jk generations`, `jk apply`, `jk confirm`, `jk rollback` | `listGenerations`, `getGeneration`, `createGeneration`, `activateGeneration`, `confirmGeneration`, `rollbackGeneration`, `discardGeneration` | `generations.list()`, `.current()`, `.get(gen)`, `.apply(bundle, *, confirm=None, dry_run=False)`, `.confirm(gen, *, reason)`, `.rollback(to=None, *, reason)` | same | viewer / admin | 1.0 |
| apps | `jk app start/stop/restart/logs` | `listApps`, `getApp`, `startApp`, `stopApp`, `restartApp` | `apps.list()`, `.get(name)`, `.start(name)`, `.stop(name, *, reason)`, `.restart(name)` | same | viewer / operator | 1.0 |
| instances | `jk status --instances`, `jk explain <i>` | `listInstances`, `getInstance` | `instances.list()`, `.get(name)` | same | viewer | 1.0 |
| logs | `jk logs` | `listLogs` | `logs.query(*, app=None, since=None, level=None)` | `logs.query(o)` | viewer | 1.0 |
| intents | `jk intents`, `jk intent approve/reject` | `listIntents`, `getIntent`, `approveIntent`, `rejectIntent` | `intents.list()`, `.get(id)`, `.approve(id, *, reason)`, `.reject(id, *, reason)` | same | operator | 1.0 (empty until 1.1) |
| any other | — | any | `request(operation_id, *, params=None, body=None)` | `request(opId, o)` | that of the operation | 1.0 |

`apply` composes `findMissingBlobs`, `startBlobUpload`/`uploadBlobChunk`/`finishBlobUpload`, `createGeneration` and `activateGeneration`; `rollback` first asks for the `ETag` (`getGeneration` of `current`) for the `If-Match`.

Rules that the hand-written layer enforces **before** sending:

- **`ttl` and `reason` are required** in `write` and `call` (16 A10, D19): in Python, *keyword-only* arguments without a default (a `TypeError` if they are missing); in TS, required properties of the type; in Rust, the *builder* does not build without them. A `ttl` above `max_manual` is rejected by the node (`J-E1102`).
- **`dry_run`** calls the preview operation (`previewManualWrite`, `previewMethodCall`) and returns what `jk write` shows before asking for confirmation: the envelope, the active level and the predicted result (07 §4). The write that follows carries `If-Match` with the generation the preview saw. The interactive confirmation belongs to `jk`, and not to the library.
- `level` omitted: `manual:4` (D19, ref/09 §5.6). A level occupied by another principal is `J-E1105`; `takeover=True` replaces it (A13).
- `apply` sends the generation *bundle* (manifest, lock, contracts, artifacts) through the chunked upload of ref/09 §5.13 (ref/00 §10) and returns the pending generation.
- `history` and `export` ask for `application/x-ndjson` and paginate through the `Link` header (ref/09 §3.5). The node produces neither Arrow nor Parquet (ref/09 §12): the library converts the NDJSON to `pandas`/Arrow/Parquet on the client side.

---

## 24. Types, values and errors

### 24.1 Samples and time

- A sample is the JSON object of ref/00, §4.2, with the same fields (`path`, `value`, `unit`, `quality`, `reason`, `ts_src`, `ts_acq`, `seq`, and `mono_ns` with `mono=True`). In Python, `Sample` (pydantic); in TS, `interface Sample`. In field names, the libraries' API follows the style of the language (`tsAcq` in TS), and the values (`"comm_failure"`) are those of the JSON.
- `Sample.get()` raises `QualityError` (`J-E0800`) if the quality is not `good`; `get_or(d)`/`getOr(d)`; `value` is the value as it came. The same rule as for apps: **the library never returns just a number from a `bad` sample**.
- UTC instants are `UtcTime` (`.ns`, `.isoformat()`, `.datetime()` in Python; `.ns: bigint`, `.toISOString()`, `.toDate()` in TS), because Python's `datetime` and JS's `Date` lose the nanoseconds of ref/00, §3.
- `i64` and `u64` in TS are `bigint`, also when the JSON carries them as a number (ref/00 §4.1). In Python, `int`.
- `bits` carry `bits` with the active names (ref/00 §4.1); `enum`s carry the variant name.

### 24.2 Input values

`write("pcs.p_set", 50)` writes 50 in the signal's unit; `write("pcs.p_set", "50 kW")` sends the quantity literal, and the node converts (ref/00 §4.1; a wrong dimension is `J0107`, returned as an API error). Durations accept literals (`"10m"`) and, in Python, `timedelta`; in Rust, `std::time::Duration`. Instants accept RFC 3339, `UtcTime`, `datetime` with a time zone (Python) or `Date` (TS); a `datetime` without a time zone is `TypeError`, because there is no implicit local time.

### 24.3 Errors

```python
class ApiError(JokerClientError):          # application/problem+json response (ref/00 §8)
    code: str                              # "J-E1203", "J0107", …
    title: str
    detail: str
    status: int
    spans: list[Span] | None
    exit_code: int                         # the jk exit code (07 §8) for this error

class JokerClientError(Exception):         # client-side errors (J-E088x)
    code: str
```

```ts
export declare class JokerClientError extends Error { readonly code: string; readonly exitCode: number }
export declare class ApiError extends JokerClientError {
  readonly title: string; readonly detail: string; readonly status: number; readonly spans?: Span[];
}
```

A response that does not comply with the generated schema (required field missing, wrong type) is `J-E0884`, with the `operationId` and the path of the field; extra fields are never an error. `exit_code`/`exitCode` follows document 07, §8: 2 for validation errors (`J01xx`–`J06xx`), 3 for refusals (403, envelope, signature), 4 for timeout or unreachable node (`J-E0881`), 5 for expired confirmation, 1 for the rest. It is what `jk` uses to exit, and what a CI script can use too.

### 24.4 Anti-replay, retries and idempotency

- Each mutating request carries the headers `Joker-Nonce` (ULID) and `Joker-Timestamp` (UTC, RFC 3339) (ref/00 §9, ref/09 §2.5), generated by the library, also on the Unix socket.
- **Reads** (`GET` and subscriptions): up to 3 retries with exponential *backoff* (0.2 s → 1.6 s) on a connection error and on 503.
- **Writes, methods, alarms, generations:** a request is only repeated if it **did not reach the node** (connection error before sending) or on a 503 with `Retry-After`. Each attempt carries a new `nonce`, because a repeated `nonce` is refused as a replay. Repeating a manual write is safe: the same writer, level and value only renew the TTL.
- **Actions:** the idempotency key is kept across attempts (generated by the library if not given, and returned in the result), and the node returns the previous result within the `dedup_window` (16 E9).
- A 4xx is never repeated.
- **Clock:** with the node's clock `synced`, a `Joker-Timestamp` outside ±30 s is refused (`J-E1021`); with `holdover` or `unsynced`, the node checks only the nonce and replies with `Joker-Clock` (D16, ref/09 §2.5). On `connect`, the library compares its clock with the response's `Date` header and, above a 10 s difference, emits the warning `J-E0885` in the log; a `J-E1021` carries the same hint.
- Default timeouts: connection 5 s, request 30 s, history and export 120 s; all configurable.

---

## 25. Subscriptions

```python
for msg in jk.subscribe(signals=["meter.**", "pcs.p_set"], alarms=True, generations=True):
    match msg:
        case SampleMsg(sample=s): ...
        case AlarmMsg(alarm=a): ...
        case GenerationMsg(generation=g): ...
        case Gap(stream=st, lost=n): ...           # there was a drop; what was lost is in the historian
```

```ts
for await (const msg of jk.subscribe({ signals: ["meter.**", "pcs.p_set"], alarms: true })) {
  if (msg.type === "sample") { /* msg.sample */ } else if (msg.type === "gap") { /* msg.lost */ }
}
```

- A single WebSocket connection (`websocket`, subprotocol `joker.v1`, ref/09 §6) with one subscription per topic: `signals`, `alarms`, `events`, `generations`, `intents`, `logs`, and the others of §6.4 of ref/09. The server's `data`, `resync` and `gap` messages arrive as `SampleMsg`, `AlarmMsg`, …, `Resync` and `Gap`.
- **Automatic reconnection** with the `resume` of ref/09 §6.7: the library keeps the `session` of the `welcome` and the last `seq` of each subscription, and resumes within the `resume_window` (60 s). If the node no longer has what is missing (`resume_unavailable`, `J-E1907`, or a new `boot_id`), the library delivers a `Gap` (and not an exception) with the number lost and subscribes again, and logs `J-E0886`. Telemetry never blocks the node (ADR 0013): a slow client drops messages, counted.
- Local *buffer* of 10,000 messages; above that, the library discards the oldest and delivers a `Gap`.
- The synchronous Python client returns a blocking iterator (the connection runs in a thread of the library).

---

## 26. Examples

### 26.1 Notebook: from the historian to pandas

```python
from joker_client import Client

jk = Client.from_config("lisboa-01")
info = jk.info()
print(info.node, info.generation, info.version)

h = jk.history(["meter.p_total", "pcs.p_ac", "bms.soc"],
               start="2026-09-23T00:00:00Z", end="2026-09-24T00:00:00Z", step="1m")
df = h.to_pandas()                          # DatetimeIndex index, UTC (ns); columns (path, field)
power = df.xs("value", axis=1, level="field")
ok = df.xs("quality", axis=1, level="field") == "good"
power.where(ok).resample("15min").mean().plot()        # the values that are not good become NaN, in plain sight

decisions = jk.decisions.query(output="pcs.p_set", start="2026-09-23T17:00:00Z",
                               end="2026-09-23T19:00:00Z").to_pandas()
decisions["outcome"].value_counts()
```

- `to_pandas()` requires the `joker-client[pandas]` extra (`J-E0888` without it). The fields per path are `value`, `quality`, `reason` and `unit` (the latter as an attribute, in `df.attrs["units"]`). `layout="long"` gives one row per sample (`ts`, `path`, `value`, `quality`, `reason`).
- **Quality always comes with the value.** There is no option to throw it away; whoever wants only the good ones filters, as above.
- The library asks for the history in NDJSON (`Accept: application/x-ndjson`, ref/09 §3.5) and builds the `DataFrame` in chunks; the node does not produce Arrow (ref/09 §12).
- Size guard: above 5,000,000 points, `history()` fails with `J-E0889` before asking, with the estimated number and the suggestion of `step` or `export()` to Parquet. `max_points=` changes the limit.

### 26.2 Commissioning script in TypeScript

It tests the curtailment of each inverter in steps, with manual writes with a TTL, and confirms the pending generation only if everything responded (example 01, §7).

```ts
// commission.ts — node --experimental-strip-types commission.ts
import { setTimeout as sleep } from "node:timers/promises";
import { ApiError, Client } from "@joker/client";

const jk = await Client.fromConfig("lisboa-01");
const gen = await jk.generations.current();
if (gen.state !== "confirming") {
  throw new Error(`generation ${gen.gen} is in ${gen.state}; nothing to confirm`);
}

type Row = { inv: string; pct: number; outcome: string; p_kw: number | null };
const rows: Row[] = [];
try {
  for (const inv of ["inv1", "inv2", "inv3"]) {
    for (const pct of [100, 60, 30, 0]) {
      const w = await jk.write(`${inv}.w_max_lim_pct`, `${pct} %`, {
        ttl: "5m", level: "manual:4", reason: "commissioning: curtailment steps",
      });
      await sleep(10_000);
      const [p] = await jk.read(`${inv}.ac.p`);
      rows.push({ inv, pct, outcome: w.outcome.outcome, p_kw: p.isGood() ? Number(p.value) : null });
    }
    await jk.release(`${inv}.w_max_lim_pct`, { level: "manual:4" });
  }
} catch (e) {
  if (e instanceof ApiError) console.error(`${e.code}: ${e.detail}`);
  throw e;                                   // the 5 min TTLs return the outputs to the apps on their own
}

console.table(rows);
if (rows.every((r) => r.outcome === "accepted" && r.p_kw !== null)) {
  await jk.generations.confirm(gen.gen, { reason: "commissioning: inverter curtailment OK" });
} else {
  console.error("not confirmed: the confirmation window expires and the node rolls back (ADR 0015)");
  process.exitCode = 5;
}
```

The script does not need to clean anything up if it fails halfway: each manual write has a TTL (A10) and expires on its own, and the generation goes back if the confirmation does not arrive.

### 26.3 Rust, in `jk`

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
render_envelope(&preview);                   // what jk shows before asking for confirmation
if confirm()? {
    let report = jk.write(preview.into_request().dry_run(false)).await?;
    println!("{}", report.outcome);
}
```

---

# Part C — Behavior, stress, decisions

## 27. Expected behavior

| Topic | Rule | Source |
|---|---|---|
| quality | the accessors follow the table of 16 §3.1 in the SDKs and in the client libraries; no API returns only the value of a sample that is not `good` other than `get_or`, `get_uncertain` or `sample` | 16 §3.1, invariant 4 |
| read in the cycle | from the latch; stable within the tick | 02 §6.1 |
| read in async | from the slot; `snapshot` for coherence | 02 §6.4 |
| write in the cycle | proposal; the last one of the tick wins; `last_outcome` in the next tick | 16 A4, 02 §7.1 |
| write in async | proposal to the owning group; `await` returns the final result | 01 §5 |
| cycle and async of the same app | the same writer in the arbiter; the cycle proposal wins over the async one in the same tick | 16 A1, A4 |
| task errors | `JokerError` = `Err` (proposals remain); another exception = `panic` (`J-E0816`: the tick's proposals discarded, counts towards the crashloop, without recreating the instance); Wasm trap = trap | ref/04 §2.6.1, 01 §9; §9.3 |
| class methods | record expanded by the core in the order of the calls; `pending()` = `None` | 17 §4.1 |
| actions | async only; idempotency key; `dedup_window` | 02 §8, 16 E9 |
| determinism | in the cycle, clocks = latch, PRNG seeded by the host, canonical NaNs, no I/O; the same trace gives the same proposals | ADR 0007, 09 attack 10 |
| hot swap | the new instance is created alongside; the state goes through `export-state`/`migrate` | 01 §9 |
| manual writes by the clients | TTL and reason required; `nonce` and `ts` always | 16 A10, ref/00 §9 |
| compatibility | tolerant readers in the client libraries; strict files written by people (`[tool.joker]`, `"joker"`) | ref/00 §1 |

---

## 28. Stress

**E1 — State in `self` or in a global, in Python.** Someone writes `self.last_plan = plan` in an async task and reads it in the cycle. *What happens:* `J-E0803` at the first assignment, with the explanation of the two instances and the pointer to `Internal`. With `global last_plan`, there is no error at runtime (the cycle always sees the initial value); `jk build` warns (`J-E0858`). *Broke* in the first version, in which `self` was a normal object: the app "worked" in the native tests and failed in production. **Fixed:** `self` read-only, and the harness always runs the real `.wasm`, with the two instances.

**E2 — Names in `camelCase` in TS.** The example of document 04, §4.1, declares `zoneTemp`. *What happens:* `J-E0857` in `jk build`, with the suggestion `zone_temp`. TS invites `camelCase`, but user names appear in the manifest, in the CLI, in the historian and in the other languages (ref/00 §2). **Fixed** in the example (finding A1).

**E3 — GC pause in a TS task at 100 ms.** A task that allocates a lot has a 15 ms pause now and then. *What happens:* the default `wcet` is 20 ms and the epoch deadline is 40 ms, and the pause fits. `jk bench` measures the **maximum** (and not p99.9) and records it in the lock, and the admission uses it. A pause above the deadline interrupts the task, the tick's proposals are lost and the leases carry on (01 §9). *Broke* when the admission used p99.9 as for Rust: a GC tail outside p99.9 gave missed deadlines in the field. **Fixed:** maximum in tier 2 (§19).

**E4 — `try: … except Exception: pass` around the control.** The quality exception is swallowed and the task continues with the value of a local variable from the previous iteration. *What happens:* it is valid code, and the SDK cannot prevent it. `jk build` warns (`J-E0863`) when a cycle task has a generic catch around calls to `get()`, and the SDK documentation opens with the right path (`get_or` or letting the exception out).

**E5 — An arbitrary exception halfway through the tick, after two `set()`s.** A `ZeroDivisionError`. *What happens:* the two proposals are discarded, as in a Rust `panic` (01 §9), and the error counts towards the crashloop. The instance is **not** recreated (§9.3). *Broke* in the first version, which treated every exception as `Err` and let half-computed proposals through. **Fixed:** only `JokerError`s keep the proposals.

**E6 — Numbers above 2⁵³ in TS.** A `u64` energy counter in Wh and instants in ns. *What happens:* they arrive as `bigint`, in the apps and in the client library, also when the JSON carries them as a number. A `number` on an `i64` port is a type error; in untyped JS, `LimitError` (`J-E0806`) if it is not a safe integer. There is never silent rounding.

**E7 — A class member called `name`, or `from`.** *What happens:* `name` is rejected by `jkc`, because the names of the handle API are reserved (ref/00 §2; finding A8, resolved). `from` gets through: the Python generator appends `_` (`p.from_()`) and `jkc` warns; the member's name in the manifest and in the `.jkc` does not change.

**E8 — Async API in the cycle, in Python.** `self.grid_power.history(...)` inside a cycle `def`. *What happens:* `J-E0859` in `jk build` (static analysis of the known calls: `history`, `write`, `invoke`, `kv.*`, `secrets.*`, `snapshot`, `subscribe`, action methods), and `J-E0901` at runtime if it escapes the lint (for example, through `getattr`). An `await` in a `def` does not even compile. In TS, it is a type error.

**E9 — Side effects on import.** `START = time.time()` at the top of the module, or `if os.environ.get("SITE") == "porto": …` around a port. *What happens:* the clock in the declaration is `J-E0810`. `os.environ` differs between the native execution and the pre-initialization, and the registry diverges: `J-E0862`. A `random` seeded at import would be the same in every instance because of the *snapshot*: the SDK seeds it again at `init`. *Broke* in the first version, without the digest comparison: the app had ports in `joker:meta` different from those the code used, and the handles got swapped. **Fixed:** the comparison in step 5 of §18.4.

**E10 — Crossed versions.** (a) A TS app generated for `joker:runtime@1.0.0` on a 1.3 runtime: it runs (ADR 0022). (b) A new `@joker/sdk` that generates `@1.3.0` sent to a 1.0 node: rejected **before** the deploy by the agent or by `jk apply`, through the `wit` of `joker:meta` (M21); `jk build --world joker:runtime@1.0.0` (or `world` in `[tool.joker]`) generates for the old node, and the 1.3 functions raise `UnavailableError`. (c) A `joker-client` 1.3 against a 1.0 node: the operations the node does not have give `J-E0883` before the request, and the rest works.

**E11 — Unstable network in a commissioning script.** The connection drops in the middle of a `write` and of an `invoke`. *What happens:* the write is repeated with a new `nonce` (the node would refuse the old one as a replay), and it is safe because it only renews the level; the action is repeated with the **same** key, and the node returns the result of the first one. With the laptop's clock 2 minutes ahead, the node refuses everything; the error carries the API code and the hint `J-E0885`. *Broke* in the initial design, which reused the `nonce` across attempts.

**E12 — A notebook asks for one raw week of 5,000 signals.** *What happens:* the estimate exceeds 5 × 10⁶ points and `history()` fails with `J-E0889` before asking, with the suggestion of `step` or of `export()` to Parquet (the library reads the paginated NDJSON and writes the Parquet in chunks, without keeping everything in memory; the node produces neither Parquet nor Arrow, ref/09 §5.5). A large request within the limit arrives in NDJSON pages. The historian and the node never wait for the client (ADR 0013).

---

## 29. Decisions and rejected alternatives

**D1 — TS: definition object.** It gives the types of `io` per domain without code generation and works in plain JS. *Rejected:* TC39 decorators (they do not change the type of `this` per domain, and the shared `this` hides the two instances); classes with methods (the same problem).

**D2 — Python: class with descriptors and `self` as a read-only view.** It gives types in mypy/pyright and is idiomatic. *Rejected:* an `io` parameter as in document 04 (without *mapped types* in Python, `io.x` would be `Any`); loose functions with a global registry (it loses the grouping and the types).

**D3 — Exceptions as `?`.** It is the idiomatic form in both languages, and the behavior at the task boundary is that of Rust's `Err`. *Rejected:* `Result` objects in Python (nobody would use them); in TS there is `tryGet()` as an alternative, without being the main path.

**D4 — An uncaught exception is a `panic`, but without a trap.** *Rejected:* a real trap (recreating a Python instance costs up to 300 ms and loses the state of all tasks); treating it as `Err` (half-computed proposals).

**D5 — Tier 2 in the cycle, with period minimums and admission by the maximum.** *Rejected:* tier 2 only in async (it pushes slow control into tasks with neither latch nor admission); cycle without minimums (GC pauses make the fast groups inadmissible in practice, and `jk check` would only say so late).

**D6 — The packed image in tier 2 too.** *Rejected:* one `cycle-io.read` call per port, as document 09, §2.2, foresaw (ms per tick in a JS or Python engine).

**D7 — `joker_sdk`, `joker_classes`, `joker_client`, `joker_testing` in Python.** *Rejected:* `import joker` (the *namespace* exists on PyPI, and the documentation would lead people to install the wrong package).

**D8 — `bigint` for `i64`, `u64` and instants in TS.** *Rejected:* `number` (silent loss above 2⁵³); `number | bigint` depending on the magnitude (a type that changes with the value breaks the code at 104 days of *uptime*).

**D9 — Plan B for Component Model async.** If ComponentizeJS or componentize-py do not have WASI 0.3 async stable when the SDK ships, the async instance of a tier 2 app is served by a variant of the world in which the `async` functions are **blocking**, on a dedicated thread of the `app` *pool* (never in the cycle domain). The SDK keeps the same API (`await` resolves immediately), and only the concurrency between tasks of the same app is lost: a slow `history()` delays the other async tasks of that app. The world variant is the `app-blocking` of ref/04 §2.11, activated by an ADR in Phase 3. *Rejected:* postponing the TS SDK beyond v1.0; an SDK without async tasks.

**D10 — Harness as a native extension (PyO3, napi-rs) of the `joker-test` crate.** A single harness, with the real `joker-core` and wasmtime, controlled step by step. *Rejected:* a subprocess with its own protocol (slow step by step, and one more interface to specify); reimplementing the harness in each language (three semantics).

**D11 — Clients with generated types and operations and hand-written ergonomics.** *Rejected:* everything generated (no Unix socket, mTLS, WebSocket, anti-replay, or idiomatic API); everything by hand (silent drift from ref/09).

**D12 — Python client in v1.0, even though the Python SDK is from 1.2.** The client is pure HTTP, does not depend on componentize-py, and is what the data and commissioning teams ask for first (21 §4). *Rejected:* waiting for 1.2.

**D13 — `health()` computed in the SDK, with shared test vectors.** The rule is a pure function of the qualities. *Rejected, for now:* an implicit `health` query evaluated by the core in each instance, which would change document 17 and the signal registry. It remains as an alternative if divergence appears.

**D14 — Ports ordered by name in `joker:meta`.** *Rejected:* declaration order (it depends on details of the interpreter and changes with a harmless reorganization of the code).

**D15 — `version` optional in the code, `name` required.** *Rejected:* duplicating both (they get out of sync); only the package (the app's name is `snake_case` and the package's need not be).

---

## 30. Version markers

| Thing | v1.0 | 1.1 | 1.2 | 1.3 |
|---|---|---|---|---|
| TS SDK (`@joker/sdk`, `@joker/test`, `@joker/classes-*`) | **yes** | | | |
| Python SDK (`joker-sdk`, `joker-testing`, `joker-classes-*`) | API closed here | | **yes** | |
| `set_at`, `write_at` (M6) | in the types; `J-E0003` in `prod`, `J-E0601` in `dev` | **yes** | | |
| `propose`, intents (M18) | in the types; `J-E0003` in `prod`, `J-E0601` in `dev` | **yes** | | |
| `Model`, `infer`, frames, streams, blocks | in the types; `unavailable` (document 11, §"Ready for ML") | **yes** | | |
| long actions with progress (`invoke_long`) | **yes** (ref/04 §2.4) | | | |
| secrets (`Secret`) | **yes** (ref/04 §2.5.2) | | | |
| Rust, Python, TS clients | **yes** | | | |
| intents in the clients | in the operation table; `J-E0883` on a node without them | **yes** | | |
| Arrow and Parquet produced by the node | **no** (NDJSON; conversion in the client, ref/09 §12) | | | |
| TS client in the browser | | | | **yes** (Studio) |
| `deck://` (Deck tunnel) | reserved | | | **yes** |
| tier 2 apps on Joker Micro | **no** (`J0720`) | no | no | no (1.4 brings Wasm on M2 MCUs, only for Rust) |

---

## 31. New codes (`J-E0800`–`J-E0899`)

| Code | Severity | Message | Where |
|---|---|---|---|
| `J-E0800` | error | read with a quality the accessor does not accept (`QualityError`) | SDKs, clients |
| `J-E0802` | error | cycle API called in async (`dt`, `pending`, `last_outcome`, `set`, mailbox receive on the wrong side) | SDKs |
| `J-E0803` | error | assignment to the app view: state is per domain; use `st`, `Internal` or `Mailbox` | SDKs |
| `J-E0804` | error | `Internal` with a non-scalar field or above 64 B encoded | build, runtime |
| `J-E0805` | error | encoding limit: signal `string`/`bytes` > 64 B, record string > 1 KiB, list > 64, invalid idempotency key | SDKs |
| `J-E0806` | error | value outside the port type (integer range, unsafe integer, invalid duration literal, negative counter) | SDKs |
| `J-E0807` | error | unknown instance in a `Many` (name or position) | SDKs |
| `J-E0808` | error | member of a capability the instance does not have | SDKs |
| `J-E0809` | error | stable API of a world later than the generated one, unavailable in this runtime (the `@unstable` items give `J-E0003`/`J-E0601`, ref/04 §2.9) | SDKs |
| `J-E0810` | error | host call during the app declaration | build |
| `J-E0811` | error | `kv`: key or value out of bounds | SDKs |
| `J-E0813` | error | task or handler declared incorrectly (domain, signature, trigger, action without handler) | build |
| `J-E0815` | error | persistent state not encodable, above 64 KiB, or incompatible without `migrate` | runtime |
| `J-E0816` | error | uncaught exception in a task (treated as `panic`) | runtime |
| `J-E0817` | error | secret not declared | SDKs |
| `J-E0818` | error | host call in native mode (outside Wasm) | SDKs |
| `J-E0850` | error | invalid entry point: no app, more than one, not inheriting from `App`, several languages in the same directory | build |
| `J-E0851` | error | invalid parameter declaration (`range`, `default`) | build |
| `J-E0852` | error | version in the code differs from the package version | build |
| `J-E0853` | error | packaging impossible: native dependency without a `wasm32-wasi` wheel, `node:*` import | build |
| `J-E0854` | error-in-prod | tier 2 cycle task below the minimum period or with `wcet` above 50% of the period | build, `jk check` |
| `J-E0855` | error-in-prod | tier 2 on an architecture without Cranelift (armv7/Pulley) | `jk check`, `jk build --target` |
| `J-E0856` | error | requested world that the SDK cannot generate | build |
| `J-E0857` | error | invalid, reserved or repeated name in the declaration | build |
| `J-E0858` | warning | task writes to a module global variable (not shared between cycle and async) | build |
| `J-E0859` | error | async API called in a cycle task (static analysis) | build |
| `J-E0860` | error | component or `include` above the maximum size | build |
| `J-E0861` | error | toolchain missing or wrong version (Node, Python, `componentize-py`) | build |
| `J-E0862` | error | the declaration differs between native execution and pre-initialization | build |
| `J-E0863` | warning | generic exception catch around reads in a cycle task | build |
| `J-E0864` | warning | no ecosystem lock: build not reproducible | build |
| `J-E0881` | error | node unreachable or timeout (socket, TCP, SSH, socket permissions) | clients |
| `J-E0882` | error | the node refused the client identity (user certificate missing, expired or without a role) | clients |
| `J-E0883` | error | operation or API version not supported by the node | clients |
| `J-E0884` | error | node response does not conform to the schema | clients |
| `J-E0885` | warning | local clock skewed from the node (> 10 s); writes may be refused | clients |
| `J-E0886` | warning | subscription with loss (`Gap`) | clients |
| `J-E0888` | error | optional dependency missing (`joker-client[pandas]`) | clients |
| `J-E0889` | error | estimated result above `max_points` | clients |

`J-E0801`, `J-E0812`, `J-E0814`, `J-E0880`, `J-E0887` and the rest of the range stay free. The configuration and node resolution errors are those of ref/02 (`J0910`–`J0913`), used as they are. `J-E0901` (async API in the cycle) belongs to document 04 and is used as it is.

---

## 32. Dependencies on other references (resolved)

What this document asked of the other references, and how it ended up after the integration (document 22, §5 and §7).

| # | Reference | Request | Outcome |
|---|---|---|---|
| X1 | ref/04 | keys `tier`, `lang`, `engine`, `sdk` and the port types for alarms, published signals, mailboxes and secrets in `joker:meta` | **resolved:** `sdk-info` 0, 3, 4, 5; `port-kind` 4, 5, 9, 10; secrets in key 13 (§3, §5.4) |
| X2 | ref/04 | handle *i* = position in the port list | **resolved:** ref/04 §2.6.2, in the order the SDK writes (§15.1) |
| X3 | ref/04 | "substituted read" record and "task error" record | **resolved:** `NOTE` (type 6, ref/04 §3.3); the task error goes in `cycle-result.error`, with no record in the buffer (§9.3) |
| X4 | ref/04 | `Err` without proposals, crashloop without recreating the instance | **resolved:** `J-E0816` with empty `proposals`, treated as a trap without recreating the instance (ref/04 §2.6.1) |
| X5 | ref/04 | async class method with a result | **resolved:** `async-io.call` → `list<member-outcome>` (it already existed) |
| X6 | ref/04 | mailbox reception in the cycle | **resolved:** `cycle-io.mailbox-recv` (it already existed) |
| X7 | ref/04 | secrets interface | **resolved:** `secrets.reveal` (ref/04 §2.5.2) |
| X8 | ref/04 | single `run-task` with subscription in the guest | **superseded:** one `run-task` per firing, also for `on` and `on_event` (ref/04 §11); §9.2 aligned |
| X9 | ref/04 | async `write` with the final result | **resolved** (ref/04 §2.4) |
| X10 | ref/04 | latch clocks and seeded randomness | **resolved:** ref/04 §2.7; the seed derives from the generation and is not recorded in the trace (§9.1 aligned) |
| X11 | ref/04 | aggregation and quality per step in `history` | **resolved:** dominant quality per step (ref/04 §2.4); `agg` in the SDKs (§6.2) |
| X12 | ref/04 | blocking variant of the world (plan B) | **resolved:** `app-blocking` world reserved (ref/04 §2.11) |
| X13 | ref/04 | reference Rust names for parity | **resolved:** §16 |
| X14 | ref/09 | `operationId` | **resolved:** those of the OpenAPI (§23); the names proposed here were abandoned |
| X15 | ref/09 | `getInfo` with version and capabilities | **resolved:** `features` and `getVersion` (ref/09 §5.1) |
| X16 | ref/09 | `dry_run` in writes and methods | **superseded:** preview operations `previewManualWrite` and `previewMethodCall` (§23) |
| X17 | ref/09 | where `nonce` and `ts` go, and the refusal code | **resolved:** `Joker-Nonce`, `Joker-Timestamp`, `J-E1021`/`J-E1022` (ref/09 §2.5) |
| X18 | ref/09 | schema of the WebSocket messages and resume cursor | **resolved:** ref/09 §6 (`resume` with `session` and `last_seq`) |
| X19 | ref/09 | Arrow in the history; export in Parquet | **refused by ref/09:** NDJSON; Arrow and Parquet on the client side (§23, §26.1) |
| X20 | ref/09 | chunked upload of the *bundle* | **resolved:** ref/09 §5.13 |
| X21 | ref/02 | resolution of `--node` and identities | **resolved:** ref/02 §3.3.1, followed as it is (§22) |
| X22 | ref/02 | tier 2 admission by `max` | **resolved:** D15, ref/02 §2.1.13 |
| X23 | 17 | reserved member names | **resolved:** ref/00 §2 (`name`, `index`, `health`, `read`, `raw`, `has`) |
| X24 | ref/00 | `J-E08xx` also for the client libraries | **resolved:** ref/00 §8 |

---

## 33. Findings in the existing documents

All of them were accepted in the integration of the round (document 22, §7), with the resolutions of §5 where there was a conflict.

| # | Document | Problem | Proposed change | Status |
|---|---|---|---|---|
| A1 | 04 §4.1 | `inputs: { zoneTemp … }` in `camelCase`, against ref/00 §2; `io.log.info` | `zone_temp`; `import { log }` and `log.info(…)` | accepted and applied (document 22, §7) |
| A2 | 04 §4.2 | `from joker import …`; `def control(self, io)`; `sum(v for v in h) / len(h)` over samples; `io.log` | `from joker_sdk import …`; `class Tank(App)`; `def control(self)` with `self.level.get()`; `h.mean()`; `log.info` | accepted and applied (document 22, §7) |
| A3 | 04 §3.1, §3.3 | the port is called `grid`, and the `bind` of 03 §8 and of example 01 §3 binds `grid_power` (`J0121`/`J0110` in `jk check`) | rename the port to `grid_power` in 04 | accepted and applied (document 22, §7) |
| A4 | 04 §3.2 | `plan.limit_kw.min(io.limit.get())` with `get_or_default()`: before the first `replan` (up to 15 min) or with the plan expired, the limit is 0 kW and the battery discharges to cover all the import | `let limit = if stale { io.limit.get() } else { plan.limit_kw.min(io.limit.get()) };` (as in §20.1) | accepted and applied (document 22, §7) |
| A5 | 04 §3.3 | `watch_soc` uses `s.value` without looking at the quality | `if let Ok(v) = s.get() { … }` | accepted and applied (document 22, §7) |
| A6 | 14 §9.1 | the lead is chosen again in every tick by the lowest hour count: with close hours, the pumps swap the lead while the pump is running; `rotate_every` is not used | choose the lead at the start of each demand (as in §20.2) or use `rotate_every` | accepted and applied (document 22, §7) |
| A7 | 14 §6.6 | `@joker/classes/std` contradicts 17 §6 (`@joker/classes-std`); `from joker.classes.std`; `(self, io)`; `cycle("10ms")` below the tier 2 minimum | `@joker/classes-std`; `from joker_classes.std`; `def control(self)`; `"100ms"` in TS and `"500ms"` in Python, with the note of §14.3 | accepted and applied (document 22, §7) |
| A8 | 17 §3.2 | nothing prevents a member called `name`, `index`, `health`, `read`, `raw` or `has`, which collides with the API of the generated handles | reserve these names (new code in the `JC` range, of 17) | resolved: ref/00 §2 |
| A9 | 17 §6 | Python `joker.classes.std` | `joker_classes.std` (§3) | accepted and applied (document 22, §7) |
| A10 | 04 §4.3 | "Python in Phase 4" (there is no Phase 4: the Python SDK is from 1.2, documents 11 and 21); "recommended minimum period"; "units at runtime" | refer to §19 of this document: minimums as a rule (`J-E0854`), memory 64/128 MiB, units validated in `jk build` | accepted and applied (document 22, §7) |
| A11 | 09 §2.2 (last point), 01 §9 | "the `cycle-io.read/write` calls are left to the tier 2 SDKs" | the tier 2 SDKs also use the packed image; `cycle-io.read/write` only for dynamic access and for values that do not fit in `u64` | accepted and applied (document 22, §7) |
| A12 | 01 §9, 09 §2.2 versus 04 §2 | 12 B proposals (`port u16, flags u16, value u64`) versus 14 B write records (`kind, flags, port u32, value`) | fix a single layout in ref/04 and correct the other two | resolved: a single layout, that of ref/04 §3.3 (14 B records) |
| A13 | 04 §2 | the world has no secrets, no async class methods with a result, no mailbox reception in the cycle, nor the port types for alarms and published signals | add them (X1–X7 of §32) | resolved: X1–X7 of §32 |
| A14 | 01 §9, 03 §8 | default `memory.max` of 16/64 MiB for all apps | default dependent on the tier (`joker:meta` carries the suggestion, §19) | accepted and applied (document 22, §7) |
| A15 | 18 §4 (table, row "SDK figures") | "Phases 3 and 4" | "Phase 3 (TS) and 1.2 (Python)" | accepted and applied (document 22, §7) |
| A16 | 04 §3.1 | `#[alarm]` only has `raise()`/`clear()` in the examples | add `set(bool)` in ref/04 for parity with §7.2 | accepted and applied (document 22, §7) |
| A17 | 07 §4 | `jk write … --reason "…"` does not say whether the reason is required | required, as in the libraries (§23) | accepted and applied (document 22, §7) |
| A18 | — | ownership of the `@joker` organization on npm has not been confirmed (the page returns 403 to automated requests) | confirm before the first publication; alternative: `@joker-rt/*` | open: D18 requires confirming the organization before the first publication |
