# ref/04 — The WIT world `joker:runtime`, the `joker:meta` section, the Rust SDK and `joker-test`

Normative reference of the interfaces that an app sees. It sits above documents 01–05 (docs/README, ADR 0023) and applies `ref/00`. The semantics (arbiter, envelope, quality, expressions) are those of document 16 and are not repeated here.

---

## 1. Scope and sources

### 1.1 What it covers

| Surface | Where | Section |
|---|---|---|
| WIT world of the apps, `joker:runtime@0.1.0` (→ `@1.0.0` in 1.0) | `wit/joker-runtime.wit` | §2 |
| cycle ABI: input image and proposal buffer, byte by byte | this document | §3 |
| Rust SDK `joker-sdk` (imported as `joker`): macros, types, traits, Wasm and native backends | this document | §4 |
| custom section `joker:meta` | `spec/cddl/joker-meta.cddl` | §5 |
| test harness `joker-test` | this document | §6 |
| runtime errors `J-E…` seen by the apps, and compilation errors of the SDK | this document | §7, §4.16 |
| machine-readable copy of the world `joker:sim/plant` (defined in ref/03, §2.4.10) | `wit/joker-sim.wit` | §2.10 |

Out of scope: the tier 2 SDKs (ref/05), the world `joker:driver` (ref/06), the C ABI and the crate of Joker Micro (ref/08), the HTTP API (ref/09).

### 1.2 Sources

Documents 04 (all), 14 §3 and §6–7, 17 §2 and §4–6, 16 (all), 19 §3 and §5, 02 §4–8, 01 §3–5, §9 and §18, 09 (attacks 2, 6, 8 and 10), 11 (§"Ready for ML"), 21 §1 and §3, ADR 0004, 0020 and 0022, and the 39 `rust` blocks of `docs/**/*.md` (§9).

### 1.3 Connections to other references

| Reference | What it shares with this one |
|---|---|
| ref/00 | quality codes (§4.3) and result codes (§4.4), time (§3), `record error` (§8), limits (§10) |
| ref/01 | `[apps.<app>]`: `bind`, `params`, `cycles`, `secrets`, `requires_clock`, `resources.*_memory`, `net.allow`; `[instances.*] implemented_by` |
| ref/02 | `joker.lock` `[apps.<app>] wit` read from `joker:meta`; secrets declared in `joker:meta` and bound in `[apps.<app>.secrets]` |
| ref/03 | scenarios and class conformance cases, which `joker-test` executes (§6.7); literals (§2.1.3); class simulators |
| ref/05 | the tier 2 SDKs use the same world, the same packed image and the same proposal buffer (§3), the same `cycle-io` functions for dynamic access and the same `joker:meta` schema (with the keys `tier`, `lang`, `engine` and `sdk`, §5); the function-by-function correspondence is in ref/05 §16 |
| ref/06 | `joker:driver` imports `joker:runtime/log` with the same version (§2.5.3) |
| ref/08 | the native backend of the SDK (feature `backend-native`, contract `NativeApp`, ref/08 §2.11.5) uses the same proposal buffer (§3.3) and the same metadata section (§5.1) |
| ref/09, ref/10 | the API and the agent show `joker:meta` and refuse, before the deploy, worlds newer than the runtime (`J-E0001`) and unstable features in `prod` (`J-E0003`); the API shows the errors of `invoke` with the codes of §7.1 and the alarms of the apps with the id `app.<app>.<name>` (§4.2) |

### 1.4 Validation of the machine-readable files

- `wit/joker-runtime.wit` and `wit/joker-sim.wit` were validated with the WIT parser of `wasm-tools` (via `jco embed --dummy` + `jco new`), in two passes: the stable world (without the `@unstable` features) and the world with all features enabled. `wit/joker-driver.wit` (ref/06) resolves against this package.
- `spec/cddl/joker-meta.cddl` was validated with `zcbor` against example CBOR instances (the one of §5.5 and two of the apps of document 04), and it rejects instances with missing required keys or values outside the choices. `zcbor` does not support the `.regexp` control (RFC 8610, §3.8.3), which was removed only for the validation.

---

## 2. The WIT world `joker:runtime`

### 2.1 Overview

An app is a component (ADR 0002) instantiated **twice** (document 01, §5; ADR 0004): a cycle instance (thread `rt/<group>`) and an async instance (tokio `app`). The imports are all in the world; the host binds them to both instances, and those that do not belong to one of them trap.

| Interface | Cycle instance | Async instance | Phase |
|---|---|---|---|
| `types` | types only | types only | 1.0 |
| `cycle-io` | yes | trap `J-E0902` | 1.0 (`write-at`: 1.1) |
| `async-io` | trap `J-E0901` | yes | 1.0 (`write-at`, `propose`: 1.1) |
| `kv` | trap `J-E0901` | yes | 1.0 |
| `secrets` | trap `J-E0901` | yes | 1.0 |
| `log` | yes | yes | 1.0 |
| `guest` (exported) | `init`, `on-params`, `on-cycle`, `export-state`, `migrate` | `init`, `on-params`, `run-task`, `on-action`, `export-state`, `migrate` | 1.0 |
| `ml` | trap `J-E0901` | yes | **reserved, 1.1** (`@unstable(feature = ml)`) |
| `frames` | trap `J-E0901` | yes | **reserved, 1.1** (`streams`) |
| `blocks` | trap `J-E0901` | yes | **reserved, 1.1** (`blocks`) |

An export called on the wrong instance does not happen: the host never calls `on-cycle` on the async instance nor `run-task` on the cycle one. An app without async tasks has no async instance; an app without cycle tasks has no cycle instance.

Tier 2 apps (TypeScript, Python) also have cycle tasks, with minimum periods of 100 ms (TS) and 500 ms (Python) and admission by the measured **maximum** (D15, ref/05 §19). They use the same world and the same cycle ABI (§3).

### 2.2 `types`

| Type | Definition | Notes |
|---|---|---|
| `port` | `u32` | index in `init-context.ports` (§2.6.2); local to the app and to the generation |
| `instance` | `u32` | index in `init-context.instances` |
| `method` | `u16` | position of the method in the `methods` list of the device in `joker:meta` (§5) |
| `level` | `u8` | arbiter level, 1–16 |
| `quality-code` | `enum { good, uncertain, bad }` | closed forever |
| `quality-reason` | `enum` with 20 cases | order of the cases = code of ref/00 §4.3 (`none` 0 … `deadline` 19) |
| `quality` | `record { code, reason }` | |
| `value` | `variant { boolean(bool), int(s64), uint(u64), float(f64), text(string), blob(list<u8>) }` | always in the unit of the port; `enum` and `bits` as `uint`; `f32` promoted; records as CBOR `blob` (§4.10) |
| `sample` | `record { value, quality, ts-src: option<s64>, ts-hw: option<u64>, ts-acq: u64, seq: u64 }` | times of ref/00 §3 |
| `clock-quality` | `enum { synced, holdover, unsynced }` | ref/00 §6 |
| `instant` | `record { mono-ns: u64, utc-ns: s64, clock }` | |
| `clamp-kind` | `enum { range, rate }` | |
| `clamp` | `record { by: clamp-kind, applied: value }` | |
| `reject-reason` | `enum` with 8 cases | order = code of ref/00 §4.4 minus 1 |
| `write-outcome` | `variant` (below) | `none` is expressed as `option::none` |
| `error` | `record { code: string, message: string }` | ref/00 §8; `code` is a code of the registry, `message` is English text that nobody interprets |
| `change` | `record { port, sample }` | |
| `event-delivery` | `record { port, payload: list<u8>, seq: u64, at: instant, lost: u32 }` | `lost`: events dropped for this consumer since the previous delivery |
| `member-outcome` | `record { port, outcome: write-outcome }` | one expanded write of a method |

**`write-outcome` and the codes of ref/00 §4.4:**

| WIT case | Code | JSON (`outcome`) | Rust (`WriteOutcome<T>`) |
|---|---|---|---|
| (absent: `option::none`) | 0 | `"none"` | `None` in `Option<WriteOutcome<T>>` |
| `accepted` | 1 | `"accepted"` | `Accepted` |
| `clamped({ by: range, applied })` | 2 | `"clamped_range"` | `Clamped { by: ClampKind::Range, applied }` |
| `clamped({ by: rate, applied })` | 3 | `"clamped_rate"` | `Clamped { by: ClampKind::Rate, applied }` |
| `suppressed` | 4 | `"suppressed"` | `Suppressed` |
| `overridden(level)` | 5 | `"overridden"` + `level` | `Overridden { level }` |
| `deferred` | 6 | `"deferred"` | `Deferred` |
| `shadowed` | 7 | `"shadowed"` | `Shadowed` (only produced from 1.1, with app shadow; ADR 0035) |
| `rejected(reason)` | 8 | `"rejected"` + `reason` | `Rejected(RejectReason)` |
| `tripped` | 9 | `"tripped"` | `Tripped` |

`reject-reason`: `wrong-type` 1, `interlock` 2, `quality` 3, `range` 4, `atomic-group` 5, `not-owner` 6, `clock-unsynced` 7, `budget` 8.

### 2.3 `cycle-io` (cycle instance)

All functions are synchronous, do not allocate in the host and do not block. **All SDKs**, the Rust one and the tier 2 ones, use the packed path (§3; ref/05 §16, A11) and only call `read` (timestamps `ts_src`/`ts_hw`, arena values and dynamic access by name), `mailbox-send`, `mailbox-recv` and `random-u64`. The remaining functions exist for dynamic access and for values that do not fit in `u64`; they are not the normal path of any SDK.

| Function | Semantics | Packed equivalent | Errors |
|---|---|---|---|
| `read(p) -> sample` | sample from the latch snapshot of this tick; on an output, the **applied** value (02 §6.1) | entry in the samples section | port without a sample (action, event, mailbox): trap `J-E0907` |
| `write(p, v)` | appends a proposal | `WRITE` record | wrong type: the proposal comes out `rejected(wrong-type)`; port that is not an output: trap `J-E0907` |
| `relinquish(p)` | releases this writer's level right away (01 §6, rule 3) | `WRITE` with `RELINQUISH` | as `write` |
| `pending(p) -> option<value>` | last proposal of this app for `p` in this tick, made in the cycle domain (the earlier tasks of the same group count; the async proposals drained at step 2 do not) | the SDK knows it by itself | — |
| `last-outcome(p) -> option<write-outcome>` | result of the proposal of **this writer** in the most recent commit of the owning group; `none` if it had no proposal in that commit | results section | — |
| `param(p) -> value` | current value of the parameter | entry in the samples section | — |
| `call(inst, m, args)` | appends a method record (17 §4.1) | `METHOD` record | method or instance out of range: trap `J-E0907`; invalid arguments: `J-E0504` in the decision log, without trap |
| `publish(p, v)` | writes a published signal (`#[publish]`), the `bool` condition of an alarm of the app (id `app.<app>.<name>`, §4.2), an `Internal<T>` with a cycle writer (CBOR `blob`), or a `state` member of an implementation | `PUBLISH` record | wrong port: trap `J-E0907` |
| `counter-add(p, delta)` | adds to a `Counter` | `COUNTER_ADD` record | same |
| `emit(p, payload)` | emits an event; full consumer queues drop and count | `EMIT` record | payload > 8 KiB: `J-E0410`, the event is discarded |
| `note-substituted(p)` | records in the trace that the read of `p` was substituted (`get_or`, 16 §3.1) | `NOTE` record | — |
| `mailbox-send(p, msg) -> bool` | non-blocking send to the async instance; `false` if full (drop counted) | — | message > 256 B: `false` and `J-E0412` in the log |
| `mailbox-recv(p) -> option<list<u8>>` | non-blocking receive | — | — |
| `now() -> instant` | latch instant of this tick (02 §5) | image header | — |
| `dt-ns() -> u64` | effective period of the group | image header | — |
| `random-u64() -> u64` | deterministic pseudorandom (§2.7) | — | — |
| `write-at(p, v, at)` | **1.1** (`scheduled-writes`, M6) | `WRITE` with `AT` | |

### 2.4 `async-io` (async instance)

| Function | Semantics | Errors (`error.code`) |
|---|---|---|
| `read(p) -> sample` | current value of the slot, without latch (02 §6.4) | — |
| `snapshot(ps) -> result<list<sample>, error>` | coherent read (same seqlock epoch); ≤ 256 ports | `J-E0414` above the limit |
| `set(p, v)` | proposal without waiting: it goes to the inbox of the owning group; `last-outcome` gives `deferred` until the commit | — |
| `write(p, v) -> result<write-outcome, error>` (async) | proposal; the future resolves after the commit of the owning group (01 §5, the `WriteTicket`) | `J-E0206` the generation changed before the commit; `J-E0205` proposal-only writer |
| `relinquish(p)` | releases the level | — |
| `last-outcome(p)` | as in `cycle-io` | — |
| `param(p) -> value` | current value | — |
| `call(inst, m, args) -> result<list<member-outcome>, error>` (async) | class method; resolves after the commit with one result per expanded write (17 §2.1) | `J-E0504`, `J-E0505`, `J-E0206` |
| `invoke(p, args, key) -> result<list<u8>, error>` (async) | action (M19, 17 §4.2): permissive and role → deduplication by the key → serialized queue of the device → result | `J-E0301`…`J-E0313`; in the API, those of §7.1 |
| `start-action(p, args, key) -> result<action-call, error>` | `@long` action: returns an `action-call` resource right away | same; `J-E0313` if the action is not long |
| `action-call.progress() -> stream<action-progress>` | progress events | — |
| `action-call.cancel()` | requests the cancellation | — |
| `action-call.finish() -> result<list<u8>, error>` (async) | waits for the result | `J-E0308` canceled |
| `publish`, `counter-add`, `emit`, `note-substituted` | as in `cycle-io`; they go through the inbox of the owning group of the signal | as in `cycle-io` |
| `subscribe(ps) -> result<stream<change>, error>` | one change per new `seq`; ≤ 256 ports; with the stream full (64), only the last change of each port is kept, and the drops count | `J-E0414` |
| `events(p) -> result<stream<event-delivery>, error>` | own consumer queue, depth 64 | `J-E0414` |
| `mailbox-send`, `mailbox-recv` | as in `cycle-io`, in the opposite direction | — |
| `mailbox-next(p) -> list<u8>` (async) | waits for the next message | — |
| `history(q) -> result<list<sample>, error>` (async) | historian (01 §12); `step-ns = 0` only with `raw`; ≤ 10,000 points; with an aggregate, one sample per step, with the **dominant quality** of the step (01 §12) and `ts-acq` = start of the step | `J-E0401` invalid interval; `J-E0402` result too large; `J-E0403` signal without recording |
| `now() -> instant` | current instant | — |
| `sleep-until(mono-ns)` (async) | wakes at the given monotonic instant (or right away, if it has passed) | — |
| `random-u64()` | §2.7 | — |
| `report-progress(request, fraction, message)` | progress of an action that this app serves in `on-action` | — |
| `is-cancelled(request) -> bool` | the caller canceled the served action | — |
| `write-at(p, v, at)` | **1.1** (`scheduled-writes`) | `J-E0207` scheduling queue full |
| `propose(p, v, reason, ttl-ns) -> result<string, error>` | **1.1** (`intents`, M18); returns the ULID of the intent, which expires after `ttl-ns` if nobody decides it (ref/09 §5.9) | `J-E0209`, `J-E0210` |

`history-query { port, from-utc-ns, to-utc-ns, step-ns, agg }` with `agg ∈ { raw, mean, min, max, first, last, count }`: the aggregates of the historian (01 §12). `count` returns `uint`.

### 2.5 `kv`, `secrets` and `log`

#### 2.5.1 `kv` (async only)

| Function | Semantics | Errors |
|---|---|---|
| `get(key) -> result<option<list<u8>>, error>` | read | `J-E0405` invalid key |
| `set(key, value)` (async) | atomic and durable write (fsync) before resolving | `J-E0404` quota; `J-E0405`; `J-E0406` value too large; `J-E0407` storage error |
| `delete(key)` (async) | same | same |
| `keys(prefix) -> result<list<string>, error>` | in bytewise order | — |
| `commit(ops)` (async) | up to 64 operations, all or none | same, and `J-E0404` |

Limits: key 1–128 B UTF-8 without control characters; value ≤ 64 KiB; total ≤ 1 MiB per app on Linux (01 §9.1) and ≤ 16 KiB on Joker Micro (19 §3).

#### 2.5.2 `secrets` (async only)

`reveal(name) -> result<list<u8>, error>`. `name` is the local name declared by the app (§4.2) and bound in the manifest (ref/02 §2.4.7). A name not declared or not bound gives `J-E0408`; declared, bound but absent from the node's vault, `J-E0409`. The value never enters the trace, the log nor the decision log; in replay, the app receives a fixed substitute value (ref/02).

#### 2.5.3 `log`

`log(lvl, msg, fields)` and `enabled(lvl) -> bool`. It does not block: each instance has a ring of 256 entries; when full, it drops and counts in `joker.app.<app>.log_dropped`. `msg` is truncated to 1 KiB, `fields` to 16 pairs of 256 B each. Rate limit: 100 messages/s per instance (the rest count as dropped).

The interface is **self-contained** (it does not use `types`) on purpose: `joker:driver` imports it with the same package version (ref/00 §1, ref/06 §4.1).

### 2.6 `guest` (exported by the app)

#### 2.6.1 The exports

| Export | Instance | When | Deadline | Error returned |
|---|---|---|---|---|
| `init(cx) -> result<_, error>` | both | when creating the instance, before the first tick or trigger | 1 s | `Err` or deadline: the instance does not start (`J-E0909`); in a hot swap, the previous version continues (01 §9) |
| `on-params(changes) -> result<_, error>` | the async one, if it exists; otherwise the cycle one, at a tick boundary | when a new generation changes parameters (ref/01 §5.12.2) | 100 ms | `Err`: the generation is rejected for this app (`J-E0011`), and `jk apply` does a rollback (01 §15) |
| `on-cycle(task, image) -> cycle-result` | cycle | once per task and per tick, step 4 (01 §4) | deadline by epoch (01 §4) | `cycle-result.error` ends the task in this tick **without** discarding the proposals already in the buffer; a trap discards them. `error.code = J-E0816` (uncaught exception in a tier 2 SDK, ref/05 §9.3) comes with empty `proposals` and the host treats it as a trap (discards the proposals of the app in that tick and counts toward the crashloop), but **without** recreating the instance |
| `run-task(task, why) -> result<_, error>` | async | one call per firing of the trigger (§4.3.2), also in `on` and `on_event` | — | `Err`: logged and counted in `joker.app.<app>.task.<t>.errors`; `J-E0816` counts in `joker.app.<app>.task.<t>.failures` and toward the crashloop |
| `on-action(req) -> result<list<u8>, error>` | async | action of a virtual instance served by this app (17 §5.3) | the `timeout` of the action | `Err` reaches the caller as it is; a trap gives `J-E0312` to the caller |
| `export-state() -> result<list<u8>, error>` | both | hot swap of the old version | 100 ms | `Err`: the new version starts with the initial state |
| `migrate(previous, from-version) -> result<_, error>` | both | hot swap of the new version, after `init` | 1 s | `Err`: the old version continues (`J-E0914`) |

`cycle-result { proposals: list<u8>, error: option<error> }` exists because a `result<list<u8>, error>` would lose the proposals of a task that returns `Err`, and document 04 §3.2 requires keeping them. The host reads `cycle-result` after the call returns: the memory of the result must not change until the next call into the instance, and an `on-cycle` post-return, if any, does not free or reuse it (§4.13, ADR 0056).

`trigger`: `startup`; `timer(instant)` for `every` and `at`; `change(change)` for `on = "<port>"`; `event(event-delivery)` for `on_event`.

`action-request { request: u64, action: u32, args, idempotency-key, caller, deadline-mono-ns }`: `action` is the index of the member in the member list of `implements` in `joker:meta` (§5); `caller` is the id of the writer (`user:ops`, `app:station`, ref/00 §5).

#### 2.6.2 `init-context` and the port table

| Field | Content |
|---|---|
| `kind` | `cycle` or `task` |
| `mode` | `normal`, `shadow` (M16: the writes come out `shadowed`; app shadow is 1.1, and a 1.0 host never sets it, ADR 0035), `replay`, `test` |
| `app`, `app-version`, `node` | identity |
| `served-world` | world served after the adapter (§2.8) |
| `generation`, `generation-hash` | generation (ref/00 §5) |
| `seed` | seed of the generator (§2.7) |
| `ports` | the port table (below) |
| `instances` | one entry per bound class instance, and one per node of the tree of class members, depth-first (§4.8.6): `device`, `elem`, `name` (the name of the instance, or the nested path of an unnamed submember), `class-version`, `capabilities` |
| `tasks` | the tasks active in this instance, with the group and the effective period |
| `implements` | name of the virtual instance served by this instance of the app |
| `cycle-abi`, `image-len`, `proposal-capacity` | cycle ABI (§3.1) |

**The port table** is built by the host, always in the same order, and the `port` of a port is its position:

1. each port of `joker:meta` (key 6), in the order of the list, which is the one the SDK wrote (the declaration order of the fields in Rust; that of the names in the tier 2 SDKs, ref/05 §15.1); a `many` port takes `n` consecutive positions (`n` fixed in `jk check`, in the order of the `bind`: that of the list, or the lexicographic order of a selector);
2. each device (key 8), in declaration order; for each element (1, or `n` in a `Many`); for each member of the member list **of the handle** (`joker:meta`, and not of the contract at runtime), one position; a class member takes no position, and one descends into its member list, depth-first (§4.8.6);
3. the members of `implements` (key 9), in the order of the list.

Each `port-binding` states the origin (`declared`, `device-member`, `command`, `state`, `class-event`), the declaration, the element, the member, the resolved name, whether it is bound (`bound`) and where it is in the image (`slot`, `outcome`). A member that the handle knows and the instance does not have (missing capability, optional member without mapping, class of an earlier minor) has `bound = false`: the read gives `J-E0506`, and a write is discarded with `J-E0506` in the log.

Because the member list comes from `joker:meta`, an app generated against `pump@1.2` runs on a `pump@1.0` instance without misaligning indices (A11 of document 14).

### 2.7 Clock, randomness and WASI

- `now()` and `dt-ns()` of the cycle instance are those of the latch; replay is exact (02 §5).
- **Randomness:** `random-u64` uses, per instance, a xoshiro256\*\* seeded with `seed` = the first 8 bytes (LE) of `SHA-256("joker-seed" ‖ generation-hash ‖ app ‖ kind ‖ implements)`, with `kind` the WIT case name (`cycle`, `task`) and `implements` the instance name or empty, in UTF-8 and with no separators; the 256-bit state is the first four outputs of SplitMix64 from `seed` (ADR 0052). Deterministic, not recorded in the trace (it is reproduced from the generation) and **not cryptographic**.
- **WASI.** A Rust, TS or Python component almost always imports WASI interfaces (the `HashMap` of `std` asks for `wasi:random/insecure-seed`). The host satisfies these, in 0.2.x and 0.3.x, always virtualized:

| WASI interface | Cycle | Async |
|---|---|---|
| `wasi:clocks/monotonic-clock`, `wall-clock` | latch instant | current instant |
| `wasi:random/random`, `insecure`, `insecure-seed` | the same generator as `random-u64` | same |
| `wasi:cli/environment`, `stdin` | empty | empty |
| `wasi:cli/terminal-input`, `terminal-output`, `terminal-stdin`, `terminal-stdout`, `terminal-stderr` | no terminal (`none`) | same (ADR 0052) |
| `wasi:cli/stdout`, `stderr` | `log` `info` / `warn`, per line, truncated to 1 KiB | same |
| `wasi:cli/exit` | trap `J-E0905` | trap `J-E0905` |
| `wasi:filesystem` | no preopened directories | same |
| `wasi:sockets`, `wasi:http/outgoing-handler` | always `access-denied` | filtered by `net.allow` (ref/01); outside the list, `access-denied` and `J-E0413` in the log |
| `wasi:io` | yes | yes |

Any other import makes the host refuse the component at load (`J-E0006`).

### 2.8 Evolution of the enums and adapters

- `quality-reason`, `reject-reason`, `clamp-kind` and `write-outcome` only grow **at the end**. Each new case is introduced by an ADR that declares its **projection** onto the existing case that an app of an earlier world sees. Examples: a new quality reason is projected onto `none` with the same `quality-code` (ref/00 §1, rule 2: it counts by its category); a new rejection reason is projected onto the closest existing reason that the ADR indicates.
- The host knows the world of each app (key 3 of `joker:meta`) and applies the projection **before** writing the image and before returning WIT values. An app never receives a case that its world does not declare, not even through the packed path (where the codes are bytes and the type would not prevent it).
- **Adapter window:** until 1.0, the runtime serves the current world and the previous minor (M21, document 09, attack 8). From 1.0 on, it serves **all** `joker:runtime@1.y` with y ≤ its own (golden rule, ADR 0022; document 21, §1.1). A world newer than the runtime is refused before the deploy by `jk` and by the agent (`J-E0001`), and, if it reaches the node, at load.

### 2.9 Versions and reservations

- The package is `joker:runtime@0.1.0` until 1.0 and `joker:runtime@1.0.0` in 1.0 (ref/00 §1). An item without a gate exists since the first version of the package; an item added in a minor carries `@since(version = 1.y.0)`; nothing is removed in a major.
- **Reservations of 1.1** use `@unstable(feature = <name>)` in the package itself:

| Feature | What it reserves | Document |
|---|---|---|
| `ml` | interface `ml` (tensors, `infer`) | 06, M5 and M17 |
| `streams` | interface `frames` | 01 §11, M4 |
| `blocks` | interface `blocks` | 01 §11, M7 |
| `scheduled-writes` | `cycle-io.write-at`, `async-io.write-at`, flag `AT` of the `WRITE` record | 02 §7.2, M6 |
| `intents` | `async-io.propose` | 01 §6.1, M18 |

- An `@unstable` item **is not part of the stable world**: the tools generate the bindings without it, unless the feature is requested (in the Rust SDK, an `unstable-<name>` feature, §4.15). A component that imports an unstable item is refused by `jokerd` in `mode = "prod"` (`J-E0003`). In `mode = "dev"`, the 1.0 runtime binds it to stubs that return `J-E0601`, which allows developing against the planned shape.
- In 1.1, each stabilized item replaces `@unstable(feature = …)` with `@since(version = 1.1.0)`. The shape may still change before that replacement, and so **an app compiled with an unstable feature is not covered by the golden rule**. This is what allows reserving the interfaces already (document 11, §"Ready for ML") without the shape chosen today being locked in for the whole 1 series.

### 2.10 Other packages in `wit/`

| File | Package | Owner |
|---|---|---|
| `wit/joker-runtime.wit` | `joker:runtime@0.1.0`, world `app` | this reference |
| `wit/joker-driver.wit` | `joker:driver@0.1.0`, world `driver`; imports `joker:runtime/log@0.1.0` | ref/06 |
| `wit/joker-sim.wit` | `joker:sim@0.1.0`, world `plant` | ref/03, §2.4.10 (this file is the machine-readable copy; if they diverge, ref/03 prevails) |

### 2.11 Plan B for the tier 2 async (D15)

If the async of WASI 0.3 is not stable in ComponentizeJS or in componentize-py when the tier 2 SDKs come out, the package gains a second world, **`app-blocking`** (name already reserved in `wit/joker-runtime.wit`):

- the same interfaces and the same functions, with the same signatures, without `async`: `write`, `call`, `invoke`, `history`, `mailbox-next`, `sleep-until`, `kv.set`, `kv.delete`, `kv.commit` and `action-call.finish` block until the result; the `stream<T>` become resources with a blocking `next() -> option<T>`;
- the host serves the async instance of that component in a **dedicated thread of the `app` pool**, never in the cycle domain; the cycle instance is the same as that of the `app` world (its functions are already synchronous);
- only the concurrency between async tasks of the same app is lost: a slow `history()` delays the other async tasks of that app (ref/05 §29, D9);
- `joker:meta` states the world (key 3); `jk check` and the agent treat `app-blocking` as the `app` of the same version for the adapter window (§2.8).

The decision to activate plan B is an ADR in Phase 3, with the proof that the async is not ready. The Rust SDK always uses the `app` world.

---

## 3. The cycle ABI (binary layout)

The fast path of document 09, attack 2: **one call per task and per tick**, `on-cycle(task, image) -> cycle-result`. All integers are little-endian, without implicit alignment (readers use `from_le_bytes`), and reserved fields are 0 on write and ignored on read.

### 3.1 Version and negotiation

- The app declares the ABI version that its SDK speaks in `joker:meta` (key 5). v1 only has version **1**.
- The host chooses the version and communicates it in `init-context.cycle-abi`, together with `image-len` (fixed during the generation) and `proposal-capacity`.
- A new version adds fields at the end of the header (the header states its length) and new record types. A host always reads the earlier versions. An SDK that declares a version that the host does not know is refused at load (`J-E0002`).

### 3.2 The input image

```
image := header (64 B) · samples (24 B × n_slots) · results (24 B × n_outcomes) · wide section
```

**Header (64 B):**

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 1 | `abi` | 1 |
| 1 | 1 | `header_len` | 64 |
| 2 | 2 | `flags` | bit 0 `FIRST_TICK` (first tick of this instance in this generation); bit 1 `SHADOW`; bit 2 `REPLAY`; bit 3 `AFTER_SKIP` (the previous tick was skipped by `overrun = "skip"`); bits 4–15 reserved |
| 4 | 4 | `task` | index of the task in `joker:meta` |
| 8 | 8 | `tick_seq` | tick number of the group |
| 16 | 8 | `t_mono_ns` | latch instant |
| 24 | 8 | `t_utc_ns` (i64) | same, in UTC |
| 32 | 8 | `dt_ns` | effective period of the group |
| 40 | 1 | `clock` | `clock_quality` (ref/00 §6) |
| 41 | 3 | reserved | |
| 44 | 4 | `n_slots` | |
| 48 | 4 | `n_outcomes` | |
| 52 | 4 | `outcomes_off` | = 64 + 24 × `n_slots` |
| 56 | 4 | `wide_off` | start of the wide section |
| 60 | 4 | `wide_len` | |

**Sample (24 B), one per port with `slot`:** inputs, outputs (applied value), parameters, `Internal<T>`, the state of each `Alarm` of the app (signal `alarm.app.<app>.<name>.state`, §4.2), class queries, `in`/`out`/`param` members of devices and the commands of an implementation, in `slot` order.

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 8 | `value` | bits of the value (ref/00 §4.1); with `WIDE`, low 32 bits = offset from the start of the image, high 32 = length |
| 8 | 1 | `quality_code` | ref/00 §4.3 |
| 9 | 1 | `quality_reason` | ref/00 §4.3, already projected (§2.8) |
| 10 | 2 | `flags` | bit 0 `WIDE`; bit 1 `HAS_TS_HW`; bit 2 `HAS_TS_SRC`; bit 3 `CHANGED` (the `seq` changed since the previous latch of this group); bit 4 `UNBOUND`; bits 5–15 reserved |
| 12 | 4 | `seq` | low 32 bits; the SDK extends them as in document 01, §3.2 |
| 16 | 8 | `ts_acq` | monotonic ns |

Bytes 8–11, read as `u32`, have the layout of the quality word of the slot (code in bits 0–7, reason in 8–15, flags in 16–31, document 01 §3.2). `ts_hw` and `ts_src` are not in the image: `HAS_TS_HW`/`HAS_TS_SRC` tell the SDK that `sample()` must fetch them with `cycle-io.read(p)`, a call without allocation in the host.

An unbound port (`UNBOUND`) has `quality = bad(not_connected)`. A sample that never arrived has `uncertain(initial)` and `value = 0`.

**Result (24 B), one per output port with `outcome`:**

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 1 | `outcome` | code of ref/00 §4.4 (0 = `none`), already projected |
| 1 | 1 | `detail` | `rejected`: reason (1–8); `overridden`: active level; otherwise 0 |
| 2 | 1 | `winner` | active level that won (1–16); 0 = none (the output is at `safe`) |
| 3 | 1 | `flags` | bit 0 `MINE_ACTIVE` (the lease of this writer is active); bit 1 `TRIPPED`; bit 2 `OTHER_GROUP` (the owning group is another one: the result is from its last commit); bits 3–7 reserved |
| 4 | 4 | `commit_seq` | low 32 bits of the tick of the owning group in which the result was produced |
| 8 | 8 | `applied` | value applied in that commit |
| 16 | 8 | `mine` | the active proposal of this writer (`mine()`, 17 §11), valid with `MINE_ACTIVE` |

**Wide section:** the values of `string`, `bytes` and `Internal<T>` (≤ 64 B each, ref/00 §10), each one at an offset that is a multiple of 8. `string` in UTF-8, `bytes` raw, records in CBOR (§4.10). The latch copies them from the wide slots of those signals (document 01, §3.2; ADR 0062), with the sample's quality, timestamps and `seq` in the 24 B sample.

The `image-len` stays fixed during the generation: it is `64 + 24 × (n_slots + n_outcomes) + 64 × n_wide`, rounded up to 8.

### 3.3 The proposal buffer

```
buffer := header (8 B) · record*
```

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `abi` (1) |
| 1 | 1 | reserved |
| 2 | 2 | `n_records` |
| 4 | 4 | `len`: total length, including the header |

**Records** (packed, without padding; the first byte is the type):

| Type | Name | Layout | Size |
|---|---|---|---|
| 1 | `WRITE` | `kind u8 · flags u8 · port u32 · value u64` [· `data[L]`] [· `at u64`] | 14 B (+ L) (+ 8) |
| 2 | `METHOD` | `kind u8 · flags u8 (=0) · instance u32 · method u16 · len u16 · args[len]` | 10 B + len |
| 3 | `PUBLISH` | `kind u8 · flags u8 · port u32 · value u64` [· `data[L]`] | 14 B (+ L) |
| 4 | `COUNTER_ADD` | `kind u8 · flags u8 (=0) · port u32 · delta u64` | 14 B |
| 5 | `EMIT` | `kind u8 · flags u8 (=0) · port u32 · len u16 · payload[len]` | 8 B + len |
| 6 | `NOTE` | `kind u8 · flags u8 (=0) · port u32` (substituted read) | 6 B |

Flags of `WRITE` and `PUBLISH`: bit 0 `RELINQUISH` (only `WRITE`; `value` = 0); bit 1 `INLINE` (`value` = length L ≤ 64, and L bytes follow: `string`, `bytes`, or the CBOR of a record); bit 2 `AT` (only `WRITE`, **1.1**, `scheduled-writes`: `at_mono_ns` follows). Bits 3–7 reserved.

Limits: `args` ≤ 4 KiB; `payload` ≤ 8 KiB; default buffer capacity 16 KiB (`#[joker::app(proposal_buffer = "…")]`, maximum 1 MiB).

### 3.4 Order, validation and errors

1. **Order.** In a call to `on-cycle`, the records made by `cycle-io` functions (dynamic access) come first, in the order of the calls; those of the returned buffer come next, in the order of the buffer. The SDKs (Rust and tier 2) write only to the buffer.
2. **Arbiter.** Several proposals of the same writer on the same output and in the same tick: the last one wins (16, A4). The async proposals drained at step 2 come before all the cycle ones.
3. **Methods.** The `METHOD` records are expanded at step 5a, in the order of the calls (17 §4.1), with `own()` seeing the earlier proposals of the same tick.
4. **Structural validation** by the host, before applying any record: `abi`, `len` and `n_records` coherent with the size of the list; known type; reserved flags at 0; `port` within the table and with the right port type (`WRITE` → output or `out` member or virtual command; `PUBLISH` → published, alarm, `Internal` of a cycle writer, `state` member; `COUNTER_ADD` → `Counter`; `EMIT` → output event or class event; `NOTE` → port with a sample); `instance` and `method` within the tables; lengths within the limits. A structural failure **discards the whole buffer** and is a trap (`J-E0907`): it counts toward the crashloop like any other trap (01 §9).
5. **Value validation**, record by record: `bool` only 0 or 1; `enum` only declared variants; `f64` NaN or infinity; invalid UTF-8 `string`. It gives `rejected(wrong-type)` to that proposal, at step 1 of the envelope, and it is not a trap.
6. **Full buffer.** The SDK does not write beyond the capacity: the following proposals are discarded, and the task ends with `cycle-result.error = J-E0203`. Those already in the buffer go through.

### 3.5 Cost

With 60 inputs and 12 outputs (1 ms group of document 09, attack 2): 64 + 24 × 72 + 24 × 12 = 2,080 B of image, copied in one operation by the canonical ABI into a pre-reserved buffer of the guest. A class app with 720 ports (document 14, A3) has ~20 KB per tick. The result per output (24 B) is what allows `last_outcome()`, `mine()` and `winner()` without calls to the host.

---

## 4. The Rust SDK `joker-sdk`

### 4.1 Crate, dependency and backends

```toml
# apps/peak_shaver/Cargo.toml (generated by `jk app new`)
[package]
name = "peak_shaver"
version = "1.4.2"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
joker = { package = "joker-sdk", version = "0.1" }

[build-dependencies]
joker-build = "0.1"

[dev-dependencies]
joker-test = "0.1"
```

```rust
// apps/peak_shaver/build.rs (generated)
fn main() {
    joker_build::generate(); // local classes (classes/local/*.jkc) → OUT_DIR/joker_classes.rs
}
```

| Crate | Role | `no_std` |
|---|---|---|
| `joker-sdk` | the API of the apps; `use joker::prelude::*` | yes; `alloc` only in the Wasm backend and on native Linux |
| `joker-sdk-macros` | the macros; version equal to that of `joker-sdk` (`=` dependency) | — (proc-macro) |
| `joker-sdk-sys` | the `wit-bindgen` bindings of the world, the low-level macros that the SDK's macros expand to, the `critical-section` of components (ADR 0053), and the allocation-free cycle path: `cabi_realloc`, the `on-cycle` export and the global allocator (ADR 0056) | yes |
| `joker-classes-std` | handles generated by `jkc` for `classes/std/`, re-exported in `joker::classes::std` | yes |
| `joker-build` | generates the local classes in `build.rs` | — |
| `joker-test` | the harness (§6) | no |

**Backends**, chosen by the target, without different code in the app. They are two mutually exclusive Cargo features of `joker-sdk`: `backend-wasm` (the default) and `backend-native`; `jk build` chooses the right one.

| Backend | Feature | Target | Who chooses it | Contract with the host | Metadata |
|---|---|---|---|---|---|
| `wasm` | `backend-wasm` | `wasm32-wasip2` (component) | `jk build` | the exports of the world (§2.6) | section `joker:meta` (§5.1) |
| `native-linux` | `backend-native` | that of `jokerd` (`kind = "native"`, 04 §5) | `jk runtime build --features native-apps` | the trait `NativeApp` (ref/08 §2.11.5) | ELF section `.joker_meta` |
| `native-micro` | `backend-native`, with `--cfg joker_micro` | `thumbv8m.main-none-eabihf`, `riscv32imac-…` (19 §5) | `jk build --target mcu:…` | `NativeApp`, driven by `joker-micro` | ELF section `.joker_meta` |

With `backend-native`, the macro `#[joker::app]` generates `impl NativeApp` instead of the WIT exports. `NativeApp` receives the same image (§3.2) and returns the same proposal buffer (§3.3), and the async tasks are fixed-size futures driven by the host (on the MCU, by `jk_service`).

**Versions:** `joker-sdk 0.y` generates `joker:runtime@0.y.0`; from 1.0 on, `joker-sdk 1.y` generates `joker:runtime@1.y.0`. To run on older runtimes, you pin an older minor of the SDK. `jk` warns when the generated world is newer than the `[project] joker` of the manifest.

**Registration of the tasks.** The tasks (`#[joker::cycle]`, `#[joker::task]`, `#[joker::on_action]`) are not listed in `#[joker::app]`: each macro registers itself at link time (constructors in `.init_array`, run by `__wasm_call_ctors` in Wasm and by the startup generated by `jk build` on Micro) and emits its fragment of `joker:meta` (§5.2). The order of the tasks is that of the name, on both sides.

### 4.2 App and port macros

#### `#[joker::app(…)]` (on the app struct, at the crate root)

| Argument | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `name` | string | no | `CARGO_PKG_NAME` with `-` → `_` | `[a-z][a-z0-9_]{0,31}`, without the `joker` prefix (`J-E0710`) |
| `version` | SemVer string | no | `CARGO_PKG_VERSION` | SemVer (`J-E0710`) |
| `requires_clock` | `"holdover"` \| `"synced"` | no | none | the stricter of this one and the one of the manifest (ref/01 `requires_clock`) prevails |
| `secrets` | list of names | no | `[]` | local `snake_case` names; bound in `[apps.<app>.secrets]` (ref/02) |
| `optional_secrets` | list of names | no | `[]` | same, without error if not bound |
| `proposal_buffer` | size (`"16 KiB"`) | no | `"16 KiB"` | 1 KiB – 1 MiB |

The doc comment of the struct goes to `joker:meta` (key 17). The macro generates: `impl joker::App for <App>`, the views `CycleIo<'_, App>` and `TaskIo<'_, App>` (§4.5), the module `classes` with the local classes (if `build.rs` generated them), the exports of the world and the metadata fragment. A second `#[joker::app]` in the crate, or one outside the root, is `J-E0711`. The generated items have the visibility of the struct.

The fields of the struct have a declaration type (§4.4) and **one** port attribute:

| Attribute | Field type | Arguments (required ones **in bold**) | Defaults and validation |
|---|---|---|---|
| `#[input]` | `Input<T>`, `InputMany<T>` (with `many`), `EventInput<E>` (with `event`) | `unit`, `quantity`, `optional`, `many`, `min`, `max`, `event` | numeric without `unit` `J-E0703` (use `unit = "1"`); unit outside the catalog `J0109`; `many`: `min` 1, `max` 256 |
| `#[output]` | `Output<T>`, `OutputMany<T>`, `EventOutput<E>` | `unit`, `quantity`, `optional`, `many`, `min`, `max`, `event`, `group` | as `#[input]`; `group`: requested owning group (16, A11) |
| `#[param]` | `Param<T>` | `unit`, `quantity`, `range`, `default` | `range = "a..b"` is **closed** on both sides (`"a..=b"` is the same; `"..b"` and `"a.."` open); `default` out of range `J-E0706`; without `default`, the manifest must give a value (`J0142`, ref/01) |
| `#[internal]` | `Internal<T: Record>` | `writer = "async" \| "cycle"` | default `"async"`; record > 64 B `J-E0708` |
| `#[publish]` | `Published<T>`, `Counter` | `unit`, `quantity` | path `app.<app>.<field>` |
| `#[alarm]` | `Alarm` | **`severity`**, **`message`**, `delay_on`, `delay_off` | alarm with the id `app.<app>.<field>` (what the API and `jk` show, ref/09 §5.8) and the signals `alarm.app.<app>.<field>.state` and `.active` (02 §11) |
| `#[action]` | `Action<A, R>` | `long` | `A` and `R`: `()` or `Record`; bound to a device action (`"pcs.reset_fault"`) |
| `#[mailbox]` | `Mailbox<T: Record, N>` | **`to = "cycle" \| "async"`** | 1 ≤ N ≤ 64; message ≤ 256 B |
| `#[device]` | class type, `Many<C, MIN, MAX>`, `Option<C>` | `requires = [...]` | `Option<C>`: optional device; 0 ≤ MIN ≤ MAX ≤ 1024 (`J-E0717`) |
| `#[model]` | `Model` | **`inputs`**, **`outputs`** | **1.1**, feature `unstable-ml` (`J-E0718` without it) |

Types `T` admitted in scalar ports: `bool`, `i64`, `u64`, `f64`, `f32`, enums with `#[derive(joker::Enum)]` (or generated by `jkc`), `Bits<N>`, `BoundedString<N>` and `BoundedBytes<N>` with N ≤ 64. Any other (`i32`, `u8`, `String`) is `J-E0707`, with the suggestion of the right type.

Field names: `snake_case`, unique in the app. Reserved (`J-E0709`) are the port names of ref/00 §2 (`now`, `dt`, `kv`, `secrets`, `snapshot`, `subscribe`, `sleep`, `sleep_until`, `log`, `cmd`, `state`, `http`, `random`, `meta`), the `joker` prefix and the names starting with `_`; in an app that implements a class, also `events`. They are the same in all languages (ref/05 §5.5).

#### `#[joker::implements(Class, capabilities = [...])]`

On the same app struct, before or after `#[joker::app]` (the order does not matter). `Class` is a class type (`Pump`, `StatusLight`); `capabilities` are the capabilities that the implementation offers (default: none). It generates `io.cmd`, `io.state` and `io.events` (§4.8.4). Unknown class or capability that the class does not have: `J-E0715`.

#### `#[joker::state(persist, version = N)]`

On the state type of a task. `persist` makes the state survive the hot swap (§4.12); `version` (default 1) is the version of the state schema. A field that cannot be encoded (§4.10) is `J-E0720`. `#[joker::migrate(<Type>)]` on a function gives the custom migration of that type (§4.12).

#### Derives

| Derive | On | Generates | Rules |
|---|---|---|---|
| `#[derive(joker::Record)]` | struct with named fields | `impl Record`, schema in `joker:meta`, maximum encoded size | fields: scalars, `Instant`, `MonoNanos`, `UtcNanos`, `UtcSeconds`, `Duration`, `Quality`, enums, `Option<T>`, other `Record`s, `BoundedVec<T, N ≤ 64>`, `BoundedString<N ≤ 1024>` (`J-E0721`) |
| `#[derive(joker::Event)]` | same | `Record` + event marker | same |
| `#[derive(joker::Enum)]` | enum without data, variants with an explicit or implicit value (0, 1, …) | `impl Enum` (u64), `Copy`, `Eq`, schema | unique values |
| `#[derive(joker::Class)]` | struct with `#[input]`/`#[output]` fields | local class (§4.8.5) | only `Input<T>` and `Output<T>` |

The derives and the traits with the same name coexist (as in `serde`): `joker::Record` is both things.

### 4.3 Task macros

#### 4.3.1 `#[joker::cycle(…)]`

| Argument | Type | Req. | Default | Validation |
|---|---|---|---|---|
| `period` | duration | **yes** | — | `J-E0714`; it is a request (04 §3.2, `J0130`) |
| `wcet` | duration | no | measured by `jk bench` | ≤ `period` (`J-E0714`) |
| `init` | path of `fn(&CycleIo<A>) -> S` | no | `S::default()` | |
| `optional` | bool | no | `false` | disabled by `overrun = "degrade"` (01 §4) |

Accepted signatures (others: `J-E0712`, with the expected signature):

```rust
#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<PeakShaver>, st: &mut Ctl) -> Result<()>   // S: Default, or init = …
#[joker::cycle(period = "200ms")]
fn control(io: &mut CycleIo<PeakShaver>) -> Result<()>                 // stateless
```

#### 4.3.2 `#[joker::task(…)]`

Exactly one trigger (`J-E0713` with zero or two):

| Trigger | Form | Extra argument of the function | Semantics |
|---|---|---|---|
| `every` | `every = "15m"`, `jitter = "30s"` (optional) | — | periodic; uniform `jitter` from the deterministic generator |
| `on` | `on = "soc"` or `on = ["a", "b"]`; members with `"<device>.<member>"` | `s: Sample<T>` (one port) or `c: Change` (several) | at each new `seq`; changes that arrive during an execution are merged into the last one |
| `on_event` | `on_event = "detections"` or `"<device>.<event>"` | `e: E` or `d: Delivery<E>` | each event is delivered, in order, one at a time; queue of 64, drops in `Delivery::lost` |
| `at` | `at = "cron: 0 */15 * * * *"` | — | 6-field cron, UTC; requires a `synced` or `holdover` clock (without it, the firing waits) |
| `startup` | `startup` | — | once, after `init`; the other triggers of the app are only delivered after all the `startup` tasks finish |

```rust
#[joker::task(every = "15m", jitter = "30s")]
async fn replan(io: &TaskIo<PeakShaver>) -> Result<()>
#[joker::task(on = "soc")]
async fn watch_soc(io: &TaskIo<PeakShaver>, s: Sample<f64>) -> Result<()>
#[joker::task(on_event = "detections")]
async fn decide(io: &TaskIo<Inspector>, d: Detections) -> Result<()>
#[joker::task(every = "1h")]
async fn tally(io: &TaskIo<Inspector>, st: &Shared<Totals>) -> Result<()> // state between executions
```

- A task never runs in parallel with itself; different tasks of the same app interleave at the `await`s (the async instance has a single thread). A trigger that fires during an execution stays pending (just one); the ones that are lost count in `joker.app.<app>.task.<t>.coalesced`.
- `Shared<S>` (`S: Default`) is the state of an async task between executions. It is only accessed with `st.with(|s| …)` and `st.with_mut(|s| …)`, so that it never crosses an `await`.

#### 4.3.3 `#[joker::on_action(<action>)]`

In an app with `#[joker::implements]`. It serves an `action` of the implemented class.

```rust
#[joker::on_action(reset)]
async fn reset(io: &TaskIo<PumpCtl>, args: (), cx: &ActionCx) -> Result<(), ActionError>
```

`args` and the success type are the ones generated by `jkc` for the action; `cx` is optional. `ActionCx`: `key() -> &str`, `caller() -> &str`, `deadline() -> MonoNanos`, `progress(fraction: Option<f32>, message: &str)`, `is_cancelled() -> bool`. An action of the class (in an offered capability) without a handler is `J-E0716` in `jk build`.

#### 4.3.4 `#[joker::on_params]`

```rust
#[joker::on_params]
fn check(changes: &ParamChanges) -> Result<()>
```

Optional, one per app. `ParamChanges`: `iter() -> impl Iterator<Item = (&str, Value)>`, `get<T: PortValue>(&self, name: &str) -> Option<T>`. It serves to reject invalid combinations (`min > max`): the `Err` rejects the generation for this app (`J-E0011`). The new values reach the cycle instance in the image of the tick following the swap boundary, and the async one in `param()`.

### 4.4 Declaration types and what they give in each view

The fields of the app struct are **declaration types** (zero-sized): the app never constructs them. `#[joker::app]` gives each field, in each view, the right handle type:

| Declaration | In `CycleIo` | In `TaskIo` |
|---|---|---|
| `Input<T>` | `CycleInput<'_, T>` | `TaskInput<'_, T>` |
| `InputMany<T>` | `CycleInputMany<'_, T>` | `TaskInputMany<'_, T>` |
| `Output<T>` | `CycleOutput<'_, T>` | `TaskOutput<'_, T>` |
| `OutputMany<T>` | `CycleOutputMany<'_, T>` | `TaskOutputMany<'_, T>` |
| `Param<T>` | `ParamHandle<'_, T>` | `ParamHandle<'_, T>` |
| `Internal<T>` | `CycleInternal<'_, T>` (`set` only with `writer = "cycle"`) | `TaskInternal<'_, T>` (`set` only with `writer = "async"`) |
| `Published<T>` | `PublishedHandle<'_, T>` | same |
| `Counter` | `CounterHandle<'_>` | same |
| `Alarm` | `AlarmHandle<'_>` | same |
| `EventOutput<E>` | `EventOutputHandle<'_, E>` | same |
| `EventInput<E>` | unavailable (`J-E0701`) | `EventInputHandle<'_, E>` |
| `Action<A, R>` | unavailable (`J-E0701`) | `ActionPort<'_, A, R>` |
| `Mailbox<T, N>` | `MailboxEnd<'_, T>` (cycle side) | `MailboxEnd<'_, T>` (async side) |
| class `C` | `<C>Cycle<'_>` | `<C>Task<'_>` |
| `Many<C, MIN, MAX>` | `ManyHandle<'_, <C>Cycle<'_>, MAX>` | `ManyHandle<'_, <C>Task<'_>, MAX>` |
| `Option<C>` | `Option<<C>Cycle<'_>>` | `Option<<C>Task<'_>>` |
| `Model` | unavailable (`J-E0701`) | `ModelHandle<'_>` (1.1) |

Using in one view an operation that only exists in the other gives a compilation error with a code and an explanation, and not a "method not found": the methods exist in both views, with a trait bound that only the right one satisfies (`#[diagnostic::on_unimplemented]`, stable since Rust 1.78):

```
error[E0277]: J-E0701: `write_at` is only available in async tasks (TaskIo)
  --> apps/inspector/src/lib.rs:21:21
   |
21 |         io.ejector.write_at(true, t);
   |                    ^^^^^^^^ cycle tasks cannot wait for a commit
   = note: a cycle task that needs this publishes an event; an async task acts on it (doc 02 §8)
```

### 4.5 The views `CycleIo` and `TaskIo`

```rust
pub trait App: 'static {
    type Cycle<'a>;
    type Task<'a>;
    // generated by #[joker::app]; not implemented by hand
}

pub struct CycleIo<'a, A: App> { /* … */ }
impl<'a, A: App> core::ops::Deref for CycleIo<'a, A> { type Target = A::Cycle<'a>; }
impl<'a, A: App> CycleIo<'a, A> {
    pub fn now(&self) -> Instant;               // latch instant
    pub fn dt(&self) -> Duration;               // effective period of the group
    pub fn tick(&self) -> u64;                  // tick_seq
    pub fn first_tick(&self) -> bool;           // FIRST_TICK
    pub fn after_skip(&self) -> bool;           // AFTER_SKIP
    pub fn shadow(&self) -> bool;               // the app runs in shadow (M16)
    pub fn task(&self) -> &'static str;
    pub fn random(&self) -> u64;
}

pub struct TaskIo<'a, A: App> { /* … */ }
impl<'a, A: App> core::ops::Deref for TaskIo<'a, A> { type Target = A::Task<'a>; }
impl<'a, A: App> TaskIo<'a, A> {
    pub fn now(&self) -> Instant;
    pub async fn sleep(&self, d: Duration);
    pub async fn sleep_until(&self, t: MonoNanos);
    pub fn snapshot<S: Snapshot>(&self, ports: S) -> Result<S::Samples>; // tuples up to 12: (&io.a, &io.b)
    pub fn random(&self) -> u64;
    pub fn kv(&self) -> Kv<'_>;
    pub fn secret(&self, name: &str) -> Result<Secret>;                  // alloc
    pub fn shadow(&self) -> bool;
}
```

The cycle view is received as `&mut CycleIo<A>`; the async one as `&TaskIo<A>`, because several async tasks of the same app share it. All proposal operations take `&self` (the buffer is interior), which avoids borrow conflicts between handles.

### 4.6 Port handles

Bodies omitted. `T: PortValue` (§4.7). The values are always in the declared unit of the port.

```rust
// Inputs: the same in both views, except where marked.
impl<'a, T: PortValue> CycleInput<'a, T> {
    pub fn get(&self) -> Result<T, QualityError>;            // Good → Ok; otherwise Err (16 §3.1)
    pub fn get_uncertain(&self) -> Result<T, QualityError>;  // Good, Uncertain → Ok; Bad → Err
    pub fn get_or(&self, default: T) -> T;                   // not Good → default, and NOTE in the trace
    pub fn sample(&self) -> Sample<T>;
    pub fn is_bound(&self) -> bool;
    pub fn changed(&self) -> bool;                           // CHANGED
}
impl<'a, T: PortValue> TaskInput<'a, T> {
    // get, get_uncertain, get_or, sample, is_bound: as in CycleInput, on the current slot
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>;   // mean per step
    pub async fn history_agg(&self, from: Instant, to: Instant, step: Duration, agg: Agg) -> Result<History<T>>;
    pub fn subscribe(&self) -> Result<Changes<'a, T>>;       // Stream<Item = Sample<T>>
}

// Outputs.
impl<'a, T: PortValue> CycleOutput<'a, T> {
    pub fn set(&self, v: T);                                 // proposal (renews the lease, 16 A2)
    pub fn relinquish(&self);
    pub fn pending(&self) -> Option<T>;
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>>;
    pub fn get(&self) -> Result<T, QualityError>;            // applied value
    pub fn sample(&self) -> Sample<T>;
    pub fn mine(&self) -> Option<T>;                         // active proposal of this writer
    pub fn winner(&self) -> Option<Level>;                   // level that won; None = safe
    pub fn is_bound(&self) -> bool;
}
impl<'a, T: PortValue> TaskOutput<'a, T> {
    pub fn set(&self, v: T);                                 // without waiting (deferred)
    pub fn write(&self, v: T) -> WriteTicket<'a, T>;         // .await → Result<WriteOutcome<T>>
    pub fn relinquish(&self);
    pub fn last_outcome(&self) -> Option<WriteOutcome<T>>;
    pub fn get(&self) -> Result<T, QualityError>;
    pub fn sample(&self) -> Sample<T>;
    pub fn mine(&self) -> Option<T>;
    pub fn winner(&self) -> Option<Level>;
    pub async fn history(&self, from: Instant, to: Instant, step: Duration) -> Result<History<T>>;
    #[cfg(feature = "unstable-scheduled-writes")]
    pub fn write_at(&self, v: T, at: impl Into<MonoNanos>) -> WriteTicket<'a, T>;
    #[cfg(feature = "unstable-intents")]
    pub async fn propose(&self, v: T, reason: &str, ttl: Duration) -> Result<IntentId>;
}
pub struct WriteTicket<'a, T> { /* … */ }
impl<'a, T: PortValue> core::future::IntoFuture for WriteTicket<'a, T> { type Output = Result<WriteOutcome<T>>; }

// Multiple ports: order = that of the bind (list) or lexicographic (selector); fixed size in the generation.
impl<'a, T: PortValue> CycleInputMany<'a, T> {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, i: usize) -> Option<&CycleInput<'a, T>>;
    pub fn iter(&self) -> core::slice::Iter<'_, CycleInput<'a, T>>;
    pub fn names(&self) -> impl Iterator<Item = &str>;       // resolved paths
}
// CycleOutputMany, TaskInputMany, TaskOutputMany: len, is_empty, get, iter, iter_mut, names.

impl<'a, T: PortValue> ParamHandle<'a, T> {
    pub fn get(&self) -> T;                                  // a parameter always has a value
}

impl<'a, T: Record> CycleInternal<'a, T> {                   // and TaskInternal
    pub fn get(&self) -> Result<T, QualityError>;            // Uncertain(Initial) until the first write
    pub fn get_or(&self, default: T) -> T;
    pub fn get_or_default(&self) -> T where T: Default;
    pub fn sample(&self) -> Sample<T>;
    pub fn set(&self, v: T);                                 // only in the view of the declared writer
}

impl<'a, T: PortValue> PublishedHandle<'a, T> { pub fn set(&self, v: T); }
impl<'a> CounterHandle<'a> { pub fn inc(&self); pub fn add(&self, n: u64); }
impl<'a> AlarmHandle<'a> {
    pub fn raise(&self);
    pub fn clear(&self);
    pub fn set(&self, active: bool);
    pub fn state(&self) -> AlarmState;                       // alarm.app.<app>.<name>.state, from the latch or from the slot
}
impl<'a, E: Event> EventOutputHandle<'a, E> { pub fn emit(&self, e: E); }
impl<'a, E: Event> EventInputHandle<'a, E> {
    pub async fn next(&self) -> Result<Delivery<E>>;
    pub fn stream(&self) -> Result<Deliveries<'a, E>>;       // Stream<Item = Result<Delivery<E>>>
}
impl<'a, T: Record> MailboxEnd<'a, T> {
    pub fn try_send(&self, v: T) -> Result<(), Full<T>>;     // only on the sending side (mailbox-send)
    pub fn lost(&self) -> u64;                               // only on the sending side: sends refused because the queue was full
    pub fn try_recv(&self) -> Option<T>;                     // only on the receiving side (mailbox-recv)
    pub fn drain(&self) -> impl Iterator<Item = T> + '_;     // only on the receiving side: try_recv until empty
    pub async fn recv(&self) -> T;                           // only on the receiving side, in TaskIo (mailbox-next)
}
```

`Delivery<E> { pub event: E, pub seq: u64, pub at: Instant, pub lost: u32 }`. `History<T>` dereferences to `[Sample<T>]`. An `Alarm` and a `Published<T>` may be written from both views: the owning group is that of the first cycle task of the app (or `default`), and for a write from both in the same tick the cycle one wins (16, A4).

#### 4.6.1 Other types of the async API

```rust
pub enum Agg { Raw, Mean, Min, Max, First, Last, Count }       // history_agg; history() uses Mean
pub struct Change { pub port: &'static str, pub sample: Sample<Value> }   // on = ["a", "b"]
pub struct Full<T>(pub T);                                      // mailbox full: returns the message
pub enum AlarmState { Normal, ActiveUnacked, ActiveAcked, RtnUnacked, Unknown }   // ref/00 §6
pub trait Snapshot { type Samples; }                            // tuples (&h1, …, &h12) of handles with a sample

impl<'a> Kv<'a> {
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub fn get_record<T: Record>(&self, key: &str) -> Result<Option<T>>;
    pub async fn set(&self, key: &str, value: &[u8]) -> Result<()>;
    pub async fn set_record<T: Record>(&self, key: &str, value: &T) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<()>;
    pub fn keys(&self, prefix: &str) -> Result<Vec<String>>;
    pub fn batch(&self) -> KvBatch<'a>;                         // .set(k, v).delete(k).commit().await
}

pub struct Secret { /* bytes, wiped on drop; no Debug nor Display of the value */ }
impl Secret { pub fn expose(&self) -> &[u8]; pub fn name(&self) -> &str; }
```

`Changes<'a, T>` and `Deliveries<'a, E>` implement `futures_core::Stream`. `IntentId` (1.1) is a ULID in `BoundedString<26>`.

### 4.7 Value, quality and time types

```rust
pub trait PortValue: Copy + PartialEq + core::fmt::Debug + 'static { /* sealed */ }
// bool, i64, u64, f64, f32, Bits<N>, BoundedString<N ≤ 64>, BoundedBytes<N ≤ 64>, T: Enum

pub struct Sample<T> {
    pub value: T,                  // T::default() if there never was a value (Uncertain(Initial))
    pub quality: Quality,
    pub ts_src: Option<UtcNanos>,
    pub ts_hw: Option<MonoNanos>,
    pub ts_acq: MonoNanos,
    pub seq: u64,
}
impl<T> Sample<T> {
    pub fn good(&self) -> Option<&T>;
    pub fn age(&self, now: Instant) -> Duration;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quality { pub code: QualityCode, pub reason: QualityReason }
impl Quality {
    pub const GOOD: Quality;
    pub const fn good() -> Self;
    pub const fn uncertain(r: QualityReason) -> Self;
    pub const fn bad(r: QualityReason) -> Self;
    pub fn is_good(&self) -> bool;
    pub fn is_uncertain(&self) -> bool;
    pub fn is_bad(&self) -> bool;
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityCode { Good = 0, Uncertain = 1, Bad = 2 }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum QualityReason {
    None = 0, Overridden = 1, LocalOverride = 2, Stale = 3, NotConfirmed = 4, Substituted = 5,
    OutOfDistribution = 6, SensorCal = 7, ClockUnsynced = 8, Initial = 9, Frozen = 10,
    PartialWrite = 11, CommFailure = 12, DeviceFault = 13, OutOfRange = 14, ConfigError = 15,
    NotConnected = 16, Nan = 17, Tripped = 18, Deadline = 19,
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[non_exhaustive]
pub enum WriteOutcome<T> {
    Accepted,
    Clamped { by: ClampKind, applied: T },
    Suppressed,
    Overridden { level: Level },
    Deferred,
    Shadowed,
    Rejected(RejectReason),
    Tripped,
}
impl<T: PortValue> WriteOutcome<T> {
    pub fn code(&self) -> u8;                                  // ref/00 §4.4
    pub fn is_applied(&self) -> bool;                          // Accepted | Clamped
    pub fn require_applied(self) -> Result<Self>;              // otherwise Err(J-E0204)
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ClampKind { Range, Rate }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RejectReason { WrongType = 1, Interlock, Quality, Range, AtomicGroup, NotOwner, ClockUnsynced, Budget }

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Level(core::num::NonZeroU8);                        // 1–16
impl Level {
    pub fn get(self) -> u8;
    pub fn band(self) -> Band;                                 // Safety, Manual, Supervisor, Control, Optimizer, Schedule
}
// Display: "manual:3"

pub use core::time::Duration;
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct MonoNanos(pub u64);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct UtcNanos(pub i64);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)] pub struct UtcSeconds(pub i64);
impl UtcSeconds { pub const MIN: Self; pub const MAX: Self; }
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ClockQuality { Synced, Holdover, Unsynced }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Instant { pub mono: MonoNanos, pub utc: UtcNanos, pub clock: ClockQuality }
impl Instant {
    pub fn utc_seconds(&self) -> UtcSeconds;
}
// Instant ± Duration → Instant (both clocks); Instant − Instant → Duration (monotonic, saturating)
// MonoNanos ± Duration → MonoNanos; UtcSeconds ± i64 → UtcSeconds; Instant: Into<MonoNanos>

pub trait DurationExt {
    fn ns(self) -> Duration;
    fn us(self) -> Duration;
    fn ms(self) -> Duration;
    fn s(self) -> Duration;
    fn mins(self) -> Duration;
    fn h(self) -> Duration;
    fn d(self) -> Duration;
}
impl DurationExt for u64 { /* … */ }
impl DurationExt for f64 { /* … */ }        // negative or NaN → Duration::ZERO; above the maximum → Duration::MAX
```

- `200.ms()` and `io.travel.get().ms()` work: the trait is implemented for **a single** integer type (`u64`), and the literal infers it. The minute is **`mins()`**, and not `min()`, which would collide with `Ord::min` (error E0034) and with `f64::min`.
- Bounded types, without allocation: `BoundedVec<T, const N: usize>` (`push -> Result<(), T>`, `iter`, `len`, `as_slice`, `Deref<[T]>`), `BoundedString<const N: usize>` (`as_str`, `try_from(&str)`), `BoundedBytes<const N: usize>`, `Bits<const N: usize>` (`bit(n) -> bool`, `bits() -> u64`).
- `Value`: `enum Value { Bool(bool), I64(i64), U64(u64), F64(f64), Text(BoundedString<64>), Bytes(BoundedBytes<64>) }`, with `From` of all the `PortValue` types and `PartialEq<f64>`, `PartialEq<bool>`, `PartialEq<i64>`, `PartialEq<u64>`.

#### 4.7.1 Errors

```rust
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Clone, PartialEq, Eq)]
pub struct Error { /* code: Code, message: BoundedString<112> (ADR 0053) */ }
impl Error {
    pub fn new(code: Code, message: impl core::fmt::Display) -> Self;   // message truncated to 112 B
    pub fn app(message: impl core::fmt::Display) -> Self;               // J-E0050
    pub fn quality(members: MemberSet) -> Self;                         // J-E0501
    pub fn code(&self) -> Code;
    pub fn message(&self) -> &str;
}
impl core::fmt::Debug for Error { /* "J-E0303: action timed out after 5 s" */ }
impl core::fmt::Display for Error { /* same */ }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Code(/* [u8; 8], ASCII */);
impl Code {
    pub fn as_str(&self) -> &str;
    pub const fn from_static(s: &'static str) -> Self;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QualityError { pub port: &'static str, pub quality: Quality }   // J-E0101, J-E0102 or J-E0103

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ActionError { pub kind: ActionErrorKind, pub error: Error }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ActionErrorKind { Denied, Forbidden, Timeout, Device, Invalid, Busy, NotConnected, Cancelled, Conflict, Unsupported, Failed, Other }
impl ActionError {
    pub fn device(message: impl core::fmt::Display) -> Self;    // J-E0304
    pub fn invalid(message: impl core::fmt::Display) -> Self;   // J-E0305
    pub fn cancelled() -> Self;                                 // J-E0308: an on_action that saw is_cancelled()
}

// From<QualityError>, From<ActionError> (and From<InferError> with unstable-ml) for Error: `?` works.
```

A host code that the SDK does not know passes intact in `Code`: the app may compare it (`e.code().as_str() == "J-E0303"`) without the SDK having to be updated. `ActionErrorKind` is derived from the code (table of §7). `Cancelled` covers both ends of a canceled long action: the caller receives `J-E0308`, and whoever serves it in `on_action` returns `ActionError::cancelled()` after seeing `cx.is_cancelled()` (the C ABI has the counterpart `JK_ACTION_CANCELLED`, ref/08 §2.6.j).

### 4.8 Classes

#### 4.8.1 Generated names

`jkc` (document 17, §6) generates, for each class, a Rust module in `joker::classes::<package>` (registry classes) or in `crate::classes::local` (project classes, via `build.rs`):

| From the class | Rust | Example |
|---|---|---|
| class name | UpperCamelCase of the name, without dots | `pump` → `Pump`; `temp_sensor` → `TempSensor`; `meter.ac3` → `MeterAc3`; `hvac.vav` → `HvacVav` |
| handles | `<Name>Cycle<'a>`, `<Name>Task<'a>` | `LedCycle<'a>` |
| state of `read()` | `<Name>State` | `PumpState` |
| enums, records | UpperCamelCase; variants in UpperCamelCase | `StatusMode::Warn` |
| commands, state and events of an implementation | `<Name>Cmd<'a>`, `<Name>StateOut<'a>`, `<Name>Events<'a>` | `PumpCmd<'a>` |
| majors | `joker::classes::std::v<major>::<Name>`; the name without a module points to the major that the crate had when the class appeared, and never changes within the major of the crate | `joker::classes::std::v2::InverterPv` |

You may import with another name (`use joker::classes::std::HvacVav as Vav;`).

#### 4.8.2 Handles

For a class with members `in x`, `out y`, `param z`, `query q`, methods `m(args)`, events `e` and actions `a(args) -> R`:

| Operation | `<C>Cycle` | `<C>Task` |
|---|---|---|
| `x()`, `y()`, `z()`, `q()` | `Result<T>`: the member with quality `Good` (an `out` gives the applied value, 17 §11); otherwise `Err` (`J-E0101`); member without binding `J-E0506`; missing capability `J-E0502` | same, on the slot |
| `m(args)` | `()`: appends a `METHOD` record | `MethodCall` → `.await` gives `Result<MethodOutcome>` |
| `a(args)` | unavailable (`J-E0701`) | `ActionCall<'_, R>` (§4.9) |
| events | unavailable | `on_event = "<device>.<e>"`, or `events_e() -> Result<Deliveries<'_, E>>` |
| `raw()` | `<C>Raw<'_>`: the members as port handles (`raw().on` is a `CycleOutput<bool>`) | same, with `history` |
| `read()` | `Result<<C>State>`: all the members; those of capabilities and the `@optional` ones as `Option<T>`; `Err(J-E0501)` with the instance `Failed` | same |
| `health()` | `Health` | same |
| `name()`, `index()` | name of the instance, position in the `Many` | same |
| `has(cap)` | `bool`: the instance has the capability | same |

- If a method has the name of a member (in `led@1`, the `out on` and the `method on()`), the handle gives the method, and the member stays in `raw()`.
- The arguments with a unit are `f64` in the unit declared in the class (`run_at(pct: f64 %)` → `run_at(60.0)`).
- `MethodOutcome`: `outcomes() -> &[(&'static str, WriteOutcome<Value>)]`, `all_applied() -> bool`, `require_applied(self) -> Result<Self>`.
- `Health`: `Ok`, `Degraded(MemberSet)`, `Failed(MemberSet)` (17 §4.5). `MemberSet`: `iter() -> impl Iterator<Item = &'static str>`, `len()`, `is_empty()`, `Debug` with the names.
- `ClassHandle` is the common trait: `name`, `index`, `health`, `read`, `has`. It is on it (or on `<C>Cycle<'_>`) that extension traits are written (14 §6.7).

Example of what `jkc` generates for `led@1` (excerpt):

```rust
pub struct Led;                                      // declaration: #[device] led: Led
pub struct LedCycle<'a> { /* … */ }
impl<'a> LedCycle<'a> {
    pub fn on(&self);
    pub fn off(&self);
    pub fn set(&self, v: bool);
    pub fn toggle(&self);
    pub fn dim(&self, pct: f64);                     // dimmable capability; without it, J-E0502 in the decision log
    pub fn brightness(&self) -> Result<f64>;
    pub fn raw(&self) -> LedRaw<'a>;                 // raw().on: CycleOutput<'a, bool>
}
```

#### 4.8.3 `Many<C, MIN, MAX>`

The form is `Many<Pump, 1, 8>`: two const parameters. The spelling `Many<Pump, 1..=8>` of documents 04 and 14 is not valid Rust (a const generic argument that is neither a literal nor a block must go between braces), and the parser fails before the macro sees it (§10, S1).

```rust
impl<'a, H: ClassHandle, const MAX: usize> ManyHandle<'a, H, MAX> {
    pub fn len(&self) -> usize;                                  // fixed at init (14 §6.2, A10)
    pub fn is_empty(&self) -> bool;
    pub fn iter(&self) -> core::slice::Iter<'_, H>;              // stable order: that of the bind
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, H>;
    pub fn get(&self, name: &str) -> Option<&H>;
    pub fn by_name(&self, name: &str) -> Result<&H>;             // Err(J-E0503)
    pub fn index(&self, i: usize) -> Option<&H>;
    pub fn names(&self) -> impl Iterator<Item = &str>;
    pub fn healthy(&self) -> impl Iterator<Item = &H>;           // health() == Ok
    pub fn read_all(&self) -> Result<BoundedVec<H::State, MAX>>; // Err if some required one is Failed
}

pub struct PerInstance<S, const MAX: usize> { /* … */ }          // per-instance state of a Many
impl<S: Default, const MAX: usize> PerInstance<S, MAX> {
    pub fn get(&self, h: &impl ClassHandle) -> &S;
    pub fn get_mut(&mut self, h: &impl ClassHandle) -> &mut S;
}
```

`PerInstance` is what fulfills A10 of document 14: it is stored by **instance name** in `export-state`, and in a generation that adds `p4` the instances `p1`–`p3` keep their state and `p4` starts at `Default`.

#### 4.8.4 Active implementations (`#[joker::implements]`)

| In the view | Type | Content |
|---|---|---|
| `io.cmd` | `<C>Cmd<'_>` | one input handle per `out` member of the class (in the offered capabilities): the **applied** value of the virtual command, after the arbiter and the envelope (14 §7.2). Without a writer, it is the `safe` |
| `io.state` | `<C>StateOut<'_>` | one `set(v)` handle per `in` and `param` member of the class: signals published by this app (`PUBLISH` record) |
| `io.events` | `<C>Events<'_>` | one `emit(e)` per event of the class |
| actions | `#[joker::on_action]` | §4.3.3 |

A member of a capability that the implementation does not offer does not exist in `io.state` nor in `io.cmd`: using it is a compilation error.

#### 4.8.5 Local classes (`#[derive(joker::Class)]`)

A struct with `#[input]` and `#[output]` fields becomes a local class (`class_ref = "local:<Name>"` in `joker:meta`). The derive generates `<Name>Cycle<'a>` and `<Name>Task<'a>` with the **same field names**, each one with the port handle of the view (`z.temp.get()?`, `z.setpoint.set(v)`), and `impl ClassHandle`. The free behavior is written in `impl <Name>Cycle<'_> { … }` in the crate itself (14 §6.7). It is bound with `{ each, map }` (ref/01 §5.12.1).

#### 4.8.6 Class members (nested handles)

For each class member (document 17, §8), `jkc` generates a **public field** in the handle, with the handle of the member's class. It is not a method: `io.esp.boot` is a `ButtonCycle<'_>`, and the rest is the usual API.

| Member in the class | Field in `<C>Cycle` / `<C>Task` | Example |
|---|---|---|
| fixed (`member boot: std/button@1`) | `pub boot: ButtonCycle<'a>` | `io.esp.boot.clicked()?` |
| collection (`member pumps: std/pump@1[1..8]`) | `pub pumps: ManyHandle<'a, PumpCycle<'a>, 8>` (§4.8.3) | `io.station.pumps.iter()`, `io.station.pumps.by_name("p1")?` |
| `@optional`, or in a capability | `pub grid: Option<SgReadyCycle<'a>>` | `if let Some(g) = &io.hp.grid { … }` |

```rust
// what jkc generates for local/bench_io@1 (excerpt)
pub struct BenchIoCycle<'a> {
    pub status: LedCycle<'a>,
    pub boot: ButtonCycle<'a>,
    pub knob: AnalogInCycle<'a>,
    /* … */
}
impl<'a> BenchIoCycle<'a> {
    pub fn rssi(&self) -> Result<i64>;
    pub fn uptime(&self) -> Result<f64>;
    pub fn online(&self) -> Result<bool>;
    pub fn raw(&self) -> BenchIoRaw<'a>;             // only the scalar members
}
```

- The methods of a nested handle are those of the member's class, and the writes are from the writer of the app, at its level, as if the app had bound the instance of the member directly (document 16, P6): `io.hp.compressor.stop()` on a composite `heat_pump` is the same `METHOD` record as `io.comp1.stop()`.
- **On a virtual instance**, the class members are read-only, because they belong to the implementation (document 16, P6). The nested handle has the usual type (the class does not know whether the instance is virtual), but the write ports of those members are read-only in the port table: a method called on them, or a write through `raw()`, is discarded by the host with `J-E0507` in the decision log, and does not write. A class whose own methods call methods of the members cannot have virtual instances (`J0656`, in `jk check`).
- `health()` of the composite handle is that of the tree (17 §4.5); `Degraded(m)` and `Failed(m)` carry the names of the class members.
- `read()` gives `<C>State` only with the scalar members; each class member has its own `read()`.
- **Names:** a class member shares the namespace of the class (`JC0206`, `JC0207`), and so a field never collides with a generated method.
- **`joker:meta`** describes the tree: a class member is a `member` with `member-kind` 6, the reference of the class, the limits of the collection and its own list of members and methods (§5, `spec/cddl/joker-meta.cddl`). The port table (§2.6.2, step 2) descends the tree **depth-first**, in the order of the members; in a collection, per element. Each node of the tree has an entry in `init-context.instances`, with `name` equal to the name of the bound instance, or to the nested path for an unnamed submember (ref/03 §2.2.14).

### 4.9 Actions

```rust
impl<'a, A: Record, R: Record> ActionPort<'a, A, R> {
    pub fn invoke(&self, args: A) -> ActionCall<'a, R>;
}
pub struct ActionCall<'a, R> { /* … */ }
impl<'a, R: Record> ActionCall<'a, R> {
    pub fn key(self, key: &str) -> Self;                        // idempotency key; default: generated
    pub async fn start(self) -> Result<ActionHandle<'a, R>, ActionError>;   // only in `long` actions
}
impl<'a, R: Record> core::future::IntoFuture for ActionCall<'a, R> { type Output = Result<R, ActionError>; }

impl<'a, R: Record> ActionHandle<'a, R> {
    pub fn progress(&self) -> Progress<'a>;                     // Stream<Item = (Option<f32>, BoundedString<256>)>
    pub fn cancel(&self);
    pub async fn finish(self) -> Result<R, ActionError>;
}
```

- `()` implements `Record`, and so `Action<(), ()>` and `pump.reset().await` work.
- **The idempotency key** (M19, E9) generated by default is a ULID made with `now()` and the deterministic generator: it protects against retransmissions by the host, **not** against the app that calls twice. Whoever wants business idempotency passes a key of their own with `.key(…)`. The same key with different arguments within `dedup_window` is `J-E0310`.
- An action waits for a response; a cycle task that needs one publishes an event (02 §8).

### 4.10 Encoding of the records

The records of the app (`Internal<T>`, `Mailbox` messages, events, arguments and results of actions and methods) travel in **deterministic CBOR** (ref/00 §4.1, §7; D2), as a **map with integer keys**: the key of each field is its position in the schema, **counting from 0** (`joker:meta` for records of the app; the `.jkc` contract for those of classes). `Plan { target_soc, limit_kw, valid_until }` is encoded `{0: 0.62, 1: 400.0, 2: 1790244000}`. It is the same form in WIT, in Joker Link (ref/07 §6.2), in JDP and in the C ABI.

- The text interfaces (API, `jk`) show the fields by name, from the schema.
- A new field is added **at the end**; reordering or removing a field changes the version of the schema (§4.12). An `Option<T>` field at `None` is omitted.
- `f64` and `f32` are encoded with the smallest floating-point format that preserves the value (RFC 8949, §4.2.2); `Instant` is `[mono, utc, clock]`; `Quality` is `[code, reason]`; enums, the numeric value.
- `Internal<T>`: the derive computes the maximum encoded size and the SDK rejects at compile time a record above 64 B (`J-E0708`). The `Plan` of document 04 (3 fields) takes at most 31 B.

### 4.11 Log

`joker::log::{trace, debug, info, warn, error}!` (re-exported in the prelude as `log::…`):

```rust
log::warn!("forecast OOD; using the conservative plan");
log::info!("level {:.2} m", level; app_mode = "auto", pump = "p1");
```

They format into a 1 KiB buffer on the stack (without allocation; truncated) and call `log.log`. The fields after `;` are `name = value` pairs (`Display`). In an app that also depends on the `log` crate, you use `joker::log::warn!` explicitly. A `panic!` writes the message to the log (`error`) before the trap. `joker::log::enabled(Level) -> bool` is the `log.enabled` of the WIT: the macros use it so as not to format messages that the host would throw away.

### 4.12 State, hot swap and migration

- The state of a cycle task (`st`) lives in the memory of the cycle instance and starts at `Default` or at the `init` of the task. It survives between ticks and traps of other tasks of the app, but not a trap that recreates the instance (crashloop, 01 §9).
- Without `#[joker::state(persist)]`, a new version starts with the initial state. With it, the hot swap does `export-state` on the old version and `migrate` on the new one, with the `joker-state` blob of the CDDL (§5): version, app version, schema version, instance kind and one CBOR per task.
- The default migration matches the fields by name; the missing ones stay at `Default` and the extra ones are ignored. A custom migration:

```rust
#[joker::migrate(Ctl)]
fn migrate_ctl(prev: StateBlob<'_>, from_version: &str) -> Result<Ctl>
```

`StateBlob`: `version() -> u32`, `field<T: Record>(&self, name: &str) -> Option<T>`. Blob above 1 MiB: `J-E0915`.

### 4.13 No allocation in the cycle

The rule is that of invariant 6 of `AGENTS.md`, carried into the Wasm instance:

| Moment | Allocation |
|---|---|
| `init`, `migrate`, `on-params` (cycle) | allowed (initial state, tables of the SDK) |
| `on-cycle` | **forbidden**: the global allocator of the cycle instance traps (`J-E0903`) |
| canonical ABI of `on-cycle` | the image and the result use buffers reserved at `init` (`cabi_realloc` returns them); the arena values of `cycle-io.read` use a 4 KiB arena reset at each tick (ADR 0056) |
| async instance | allowed (limit: `memory.max`, `J-E0906`) |

- Everything that the cycle API returns is `Copy` or bounded (`BoundedVec`, `BoundedString`); the formatting of the log uses the stack.
- `joker-sdk-sys` implements this (ADR 0056): `init` reserves an image buffer of `image-len` bytes and a proposal buffer of `proposal-capacity` bytes; outside `on-cycle`, `cabi_realloc` lends the image buffer to the host for a request of `image-len` bytes with alignment 1, and gives heap memory for the rest; inside, it serves everything from the 4 KiB arena, and what does not fit traps with `J-E0903`. The `on-cycle` export is written by hand (not by `wit-bindgen`): the task writes the proposals into the reserved buffer, `cycle-result` points at it and at static buffers for the error (code 16 B, message 256 B), and there is no post-return.
- The global allocator of a component is the one of `joker-sdk-sys`: the system allocator outside `on-cycle`; inside, an allocation writes the log record `J-E0903: heap allocation during a cycle task` (`error`, no fields) and traps. When a call ends in a Wasm trap and that record came in that call, the host reports `J-E0903` and not `J-E0905`, even if the level filter or the rate limit drops the record. Freeing memory is allowed.
- The state of each stateful cycle task is created at `init`; the first tick does not allocate. Capacity reserved at `init` (or in the state's `Default`) can be used in the cycle; growing beyond it traps.
- This replaces the bump allocator reset at each tick of document 04, §3.2: with it, a `Vec` kept in `st` would end up pointing at memory reused in the next tick, with no error at all (§10, S3).
- `joker-sdk` is `#![no_std]`. `alloc` comes in only in the `wasm` and `native-linux` backends, and only in the async APIs that return variable-size collections (`History`, `Secret`, the `Vec` of `kv`).

### 4.14 Joker Micro (native backend)

The same app compiles for the MCU (19 §5), with the feature `backend-native` and the contract `NativeApp` of ref/08 §2.11.5. The `native-micro` backend does not copy the image: the core gives the SDK a view of the snapshot with the same layout as §3.2, and the SDK returns the same buffer as §3.3, which `joker-core` drains as on Linux. A *panic* of an app on an MCU is `jk_fatal` (D22): there is no *sandbox*.

| API | Linux (Wasm) | Native Linux | Joker Micro |
|---|---|---|---|
| everything that is cycle | yes | yes | yes |
| `set`, `write`, `relinquish`, `snapshot`, `subscribe`, events, mailbox, `emit`, methods, `invoke`, `sleep` | yes | yes | yes |
| `every`, `on`, `on_event`, `startup` | yes | yes | yes |
| `at = "cron: …"` | yes | yes | yes, with `TIME` of Joker Link (without it, it waits) |
| `kv` | 1 MiB | 1 MiB | 16 KiB |
| `secrets` | yes | yes | only on standalone nodes with secrets in the blob (ref/02 §2.4.8); otherwise `J-E0408` |
| `history` | yes | yes | **`J0720`** |
| network (`wasi:sockets`, `wasi:http`) | with `net.allow` | with `net.allow` | **`J0720`** |
| `Secret`, `History` and other collections with `alloc` | yes | yes | **`J0720`** |
| `#[joker::state(persist)]` | yes | there is no hot swap (04 §5): ignored, with a warning | **`J0720`** |
| `ml`, `streams`, `blocks`, `scheduled-writes`, `intents` | 1.1 | 1.1 | **`J0720`** |

The `J0720` are compilation errors with the same mechanism as §4.4: the methods exist, with a bound `where platform::Current: platform::Historian` that the `joker_micro` target does not satisfy:

```
error[E0277]: J0720: `history()` is not available on Joker Micro
  --> apps/level/src/lib.rs:31:18
   |
31 |     let h = io.level.history(now - 24.h(), now, 15.mins()).await?;
   |                      ^^^^^^^ the MCU has no historian
   = help: use a core window in the manifest (`avg(level, 15m)`) or move this task to a Linux node
```

**`J0721`** (warning: `f64` on an MCU without a double-precision FPU, that is, with `fpu = "none"` or `"sp"`) is emitted by `jk build --target mcu:…` from the `f64` ports and fields in `joker:meta`, from the FPU declared by the board (ref/03, §2.3.1; the enum of ref/08 §2.14) and from the software floating-point routines that it finds in the object of the app (ref/08 §2.11.5), and not by the compiler. `jk bench --target mcu` measures the real cost.

### 4.15 Unstable features

| Cargo feature | WIT feature | Gives |
|---|---|---|
| `unstable-ml` | `ml` | `#[model]`, `ModelHandle::infer`, `Tensor`, `Inference`, `InferError` |
| `unstable-streams` | `streams` | `StreamInput<F>`, `Frame` |
| `unstable-blocks` | `blocks` | `BlockInput`, `Block` |
| `unstable-scheduled-writes` | `scheduled-writes` | `write_at` |
| `unstable-intents` | `intents` | `propose`, `IntentId` |

Using one of these APIs without the feature is `J-E0718` (with the feature to enable). With the feature, the feature enters `joker:meta` (key 10) and the production `jokerd` refuses the component (`J-E0003`) until the version that stabilizes it. Planned signatures (they may change until stabilization):

```rust
#[cfg(feature = "unstable-ml")]
impl<'a> ModelHandle<'a> {
    pub async fn infer<const K: usize>(&self, inputs: [Tensor; K]) -> Result<Inference, InferError>;
}
#[cfg(feature = "unstable-ml")]
impl Tensor {
    pub fn new(dtype: DType, shape: &[u32], data: &[u8]) -> Result<Self, InferError>;
    pub fn from_features<T: PortValue>(series: &History<T>, extra: &[f32]) -> Result<Self, InferError>;
    pub fn as_slice<E: Element>(&self) -> &[E];
    pub fn shape(&self) -> &[u32];
}
#[cfg(feature = "unstable-ml")]
pub struct Inference { pub outputs: Vec<Tensor>, pub quality: Quality, pub backend: String, pub latency: Duration }
```

### 4.16 Compilation errors of the SDK

The macros and the trait bounds emit messages with a code, the location on the offending field or attribute and a help line. `jk build` shows them in rustc style and `jk explain` explains them.

| Code | When | Message (first line) |
|---|---|---|
| `J-E0701` | async API used in `CycleIo` (or cycle API in `TaskIo`) | `` `{api}` is only available in async tasks (TaskIo) `` |
| `J-E0702` | `set` on an `Internal<T>` in the view that is not that of the writer | `` `{port}` is written by the {domain} domain; declare #[internal(writer = "…")] to change it `` |
| `J-E0703` | numeric port without `unit` | `` numeric port `{port}` needs a unit; use unit = "1" for a dimensionless value `` |
| `J0109` | unit outside the catalog (16 §5) | `` unit `{unit}` is not in the v1 catalog `` |
| `J-E0705` | invalid duration literal | `` invalid duration `{lit}`; expected e.g. "200ms", "1.5s", "90m" `` |
| `J-E0706` | invalid range, or `default` outside it | `` default {v} is outside range {range} `` |
| `J-E0707` | unsupported port type | `` `{ty}` is not a port type; use {suggestion} `` |
| `J-E0708` | `Internal<T>` above 64 B, or with non-scalar fields | `` internal record `{ty}` may encode to {n} bytes; the limit is 64 `` |
| `J-E0709` | duplicate or reserved field name | `` port name `{name}` is reserved `` |
| `J-E0710` | invalid `name` or `version` | `` app name `{name}` must match [a-z][a-z0-9_]{0,31} and not start with "joker" `` |
| `J-E0711` | two `#[joker::app]`, or outside the crate root | `` a crate holds exactly one #[joker::app], at the crate root `` |
| `J-E0712` | invalid task signature | `` expected `fn(&mut CycleIo<App>, &mut State) -> Result<()>` `` (or the async one) |
| `J-E0713` | invalid trigger: zero or two, `on` for a port that does not exist, `on_event` for a port that is not an event port | `` a task needs exactly one trigger: every, on, on_event, at or startup `` |
| `J-E0714` | `period` missing, `wcet > period`, period < 100 µs | `` cycle task `{task}` needs period = "…" `` |
| `J-E0715` | `#[implements]` with an unknown class or capability | `` class `{class}` has no capability `{cap}` `` |
| `J-E0716` | class action without `#[joker::on_action]` (in `jk build`, from `joker:meta`) | `` action `{action}` of `{class}` has no handler `` |
| `J-E0717` | invalid `Many` limits | `` Many<_, {min}, {max}>: expected 0 <= MIN <= MAX <= 1024 `` |
| `J-E0718` | unstable API without the feature | `` `{api}` is reserved for v1.1; enable the `unstable-{feature}` feature `` |
| `J-E0719` | cycle state type that is not `'static` | `` cycle state `{ty}` must be 'static `` |
| `J-E0720` | `persist` state with a field that cannot be encoded | `` field `{field}` of `{ty}` cannot be persisted `` |
| `J-E0721` | record field of an unsupported type | `` field `{field}`: `{ty}` is not a record field type `` |
| `J0720` | API unavailable on Joker Micro (§4.14) | `` `{api}` is not available on Joker Micro `` |

---

## 5. The `joker:meta` section

The complete schema is in `spec/cddl/joker-meta.cddl`. Here is what the schema does not say.

### 5.1 Where it lives

| Artifact | Location |
|---|---|
| component from `jk build` | custom section `joker:meta` at component level, with **one** `joker-meta` item (canonical form) |
| component from `cargo build` | the same section in the embedded core module, with the sequence of fragments (§5.2) |
| native binary (Micro, `kind = "native"`) | ELF section `.joker_meta`, with the sequence of fragments |

The readers (`jokerd`, `jk inspect`, `jk check`, the agent) look first at component level and then in the core module. Without a section, or with one that does not validate against the CDDL, the component is refused (`J-E0004`). Key 3 (world) must match the world that the component actually imports (`J-E0005`).

### 5.2 Fragments

Each macro emits its fragment (`meta-fragment`), and the linker joins the sections with the same name. The section is therefore a **CBOR sequence** (RFC 8742) of fragments: one `app-fragment` (from `#[joker::app]`), one `task-fragment` per task and one `schema-fragment` per type with a derive.

### 5.3 Merging

1. Exactly one `app-fragment` (`J-E0004`).
2. Tasks: sorted by name (bytewise order); the index in the list is the `task` of `on-cycle` and `run-task`. Two fragments with the same name: `J-E0004`.
3. Schemas: sorted by name and deduplicated. The same name with different content: `J-E0004`. The name is the identifier of the Rust type; the schemas generated from classes have the prefix of the class (`"std/pump@1:tripped"`).
4. The result is encoded in deterministic CBOR. `jk build` rewrites the section with it; the digest of the component covers it.

### 5.4 What each consumer uses

| Consumer | Keys |
|---|---|
| `jk check` (ref/01) | 1, 2, 6 (bindings, units, ranges, `optional`), 7 (groups, `J0130`), 8 (classes, `requires`, `Many`), 9 (implementations, `J0191`), 13 (secrets, `J0927`) |
| `jk build --target mcu` | 6, 11 (`f64` types, `J0721`), 10 (unstable, `J0720`) |
| `joker.lock` (ref/02) | 2 (`version`), 3 (`wit`) |
| `jokerd` at load | 3 (world, §2.8), 5 (ABI), 10 (`J-E0003`), 6–9 (port table, §2.6.2), 16 (memory) |
| agent and Deck (ref/10) | 2, 3, 10, before the deploy |
| `jk inspect`, API (ref/09) | everything |

### 5.5 Example

The `level` app of example 00, canonical form (168 B), in CBOR diagnostic notation:

```
{
  0: 1,
  1: "level",
  2: "0.1.0",
  3: "joker:runtime@0.1.0",
  4: {0: "rust", 1: "0.1.0", 2: 0},
  5: 1,
  6: [
    {0: "level", 1: 0, 2: 3, 3: "m"},
    {0: "pump", 1: 1, 2: 3, 3: "%"},
    {0: "target", 1: 2, 2: 3, 3: "m", 7: {0: 0, 1: 3}, 8: 1.5},
    {0: "kp", 1: 2, 2: 3, 3: "1", 7: {0: 0, 1: 500}, 8: 80.0}
  ],
  7: [{0: "control", 1: 0, 2: 100000000, 3: 10000}],
  8: [],
  10: [],
  11: []
}
```

---

## 6. `joker-test`

### 6.1 Modes

| Command | What runs | When |
|---|---|---|
| `jk test` | the real `.wasm` component in wasmtime (`jk` compiles it and passes it to the test binary in `JOKER_TEST_COMPONENT`), with the real `joker-core` and a virtual clock | always in CI; it is what counts (04 §3.7) |
| `cargo test` | the same app code with the `native-linux` backend, linked into the test binary | quick debugging; the harness writes a warning that it is not the production component |

In both modes: **virtual clock**, real core (arbiter, envelope, quality), total determinism. After each tick, the harness runs the async executor until all the tasks are waiting for a timer or a trigger; the async proposals enter at step 2 of the next tick.

### 6.2 Construction

```rust
pub struct Harness<A: App> { /* … */ }
impl<A: App> Harness<A> {
    pub fn new() -> Self;
    pub fn group(self, task: &str, period: Duration) -> Self;            // period of the group of this task
    pub fn param(self, name: &str, v: impl Into<ParamValue>) -> Self;     // 400.0, true, "400 kW" (ref/03 §2.1.3)
    pub fn input(self, port: &str, s: impl Into<Series>) -> Self;         // 0.5 is Series::constant(0.5)
    pub fn internal<T: Record>(self, port: &str, v: T) -> Self;
    pub fn command(self, member: &str, v: impl Into<Value>) -> Self;      // virtual command of an implementation
    pub fn device(self, name: &str, d: SimDevice) -> Self;                // simulated instance for a #[device]
    pub fn devices(self, name: &str, ds: impl IntoIterator<Item = SimDevice>) -> Self; // Many
    pub fn safe(self, output: &str, v: impl Into<Value>) -> Self;         // default: T::default()
    pub fn envelope(self, output: &str, e: Envelope) -> Self;             // default: only the safe
    pub fn lease(self, d: Duration) -> Self;                              // default: 3 × period (16 A9)
    pub fn level(self, level: &str) -> Self;                              // default: "control:12"
    pub fn manual(self, output: &str, v: impl Into<Value>, at: Duration, ttl: Duration) -> Self; // override
    pub fn on_action(self, port: &str, r: ActionResponder) -> Self;       // response of a called action
    pub fn history(self, port: &str, s: impl Into<Series>) -> Self;       // data for history()
    pub fn kv(self, key: &str, v: &[u8]) -> Self;
    pub fn secret(self, name: &str, v: &[u8]) -> Self;
    pub fn start_utc(self, rfc3339: &str) -> Self;                        // default: "2026-01-01T00:00:00Z"
    pub fn clock(self, q: ClockQuality) -> Self;
    pub fn seed(self, seed: u64) -> Self;
    pub fn manifest(self, path: &str) -> Self;                            // envelopes and bindings of a real manifest
    pub fn scenario(self, path: &str) -> Self;                            // scenario of ref/03
}
```

`Envelope`: `Envelope::new().range(lo, hi).rate(per_s).safe(v).on_interlock_hold()`. `ActionResponder`: `ActionResponder::ok(value)`, `::err(code)`, `::after(d, value)`, `::with(|args| …)`.

### 6.3 Execution

```rust
impl<A: App> Harness<A> {
    pub fn run_for(&mut self, d: Duration);
    pub fn run_until(&mut self, t: Duration);        // virtual time since the start
    pub fn step(&mut self);                           // one tick of the fastest group
    pub fn now(&self) -> Instant;
    pub fn set_input(&mut self, port: &str, v: impl Into<Value>);
    pub fn inject(&mut self, port: &str, q: Quality, for_: Duration);
    pub fn set_param(&mut self, name: &str, v: impl Into<ParamValue>);   // calls on-params
    pub fn emit(&mut self, port: &str, e: impl Record);                  // input event
    pub fn stop_app(&mut self);                                          // both instances stop
    pub fn restart_app(&mut self);
    pub fn hot_swap(&mut self) -> Result<()>;                            // export-state + migrate
}
```

### 6.4 Results and assertions

```rust
impl<A: App> Harness<A> {
    pub fn output(&self, port: &str) -> OutputTrace;
    pub fn input_series(&self, port: &str) -> SeriesTrace;
    pub fn device(&self, name: &str) -> DeviceTrace;     // .member("on") → OutputTrace
    pub fn events(&self, port: &str) -> EventTrace;
    pub fn alarm(&self, name: &str) -> AlarmTrace;
    pub fn published(&self, port: &str) -> SeriesTrace;
    pub fn internal(&self, port: &str) -> SeriesTrace;
    pub fn actions(&self, port: &str) -> ActionTrace;
    pub fn traps(&self) -> &[Trap];                      // Trap { at, task, code, message }
    pub fn errors(&self) -> &[TaskError];                // returned Errs
    pub fn logs(&self) -> &[LogLine];
}

impl OutputTrace {
    // windows (return another OutputTrace)
    pub fn after(&self, t: Duration) -> OutputTrace;
    pub fn before(&self, t: Duration) -> OutputTrace;
    pub fn between(&self, a: Duration, b: Duration) -> OutputTrace;
    pub fn when(&self, c: Condition) -> OutputTrace;
    // on the applied value
    pub fn values(&self) -> impl Iterator<Item = Value> + '_;
    pub fn all(&self, f: impl FnMut(Value) -> bool) -> bool;
    pub fn any(&self, f: impl FnMut(Value) -> bool) -> bool;
    pub fn at(&self, t: Duration) -> Observed;
    pub fn last(&self) -> Observed;
    pub fn toggles(&self) -> usize;
    pub fn intervals(&self) -> impl Iterator<Item = Duration> + '_;
    pub fn toggles_every(&self, d: Duration) -> bool;    // all intervals equal to d
    // on the proposals and the results
    pub fn proposals(&self) -> impl Iterator<Item = (Duration, Value)> + '_;
    pub fn no_new_writes(&self) -> bool;
    pub fn outcomes(&self) -> impl Iterator<Item = (Duration, WriteOutcome<Value>)> + '_;
    pub fn levels(&self) -> impl Iterator<Item = (Duration, Option<Level>)> + '_;  // level that won each tick; None = safe
}

impl SeriesTrace {
    pub fn lt(&self, v: impl Into<Value>) -> Condition;
    pub fn le(&self, v: impl Into<Value>) -> Condition;
    pub fn gt(&self, v: impl Into<Value>) -> Condition;
    pub fn ge(&self, v: impl Into<Value>) -> Condition;
    pub fn eq(&self, v: impl Into<Value>) -> Condition;
    pub fn bad(&self) -> Condition;
    pub fn values(&self) -> impl Iterator<Item = Value> + '_;
}
// Condition: and, or, not.

pub struct Observed(/* Option<Value> */);
impl<T: Into<Value> + Copy> PartialEq<Option<T>> for Observed { /* … */ }  // assert_eq!(h.output("led").last(), Some(false))
```

The traces are not generic: a `Value` is compared with `f64`, `bool`, `i64` and `u64` (`all(|v| v == 0.0)`), and `Observed` is compared with `Option<T>`. Thus `h.output("led")` needs no type annotation. An unknown port makes a `panic!` with the list of the ports of the app.

### 6.5 Series

```rust
impl Series {
    pub fn constant(v: impl Into<Value>) -> Self;
    pub fn step(before: impl Into<Value>, after: impl Into<Value>, at: Duration) -> Self;
    pub fn ramp(from: f64, to: f64, over: Duration) -> Self;       // linear and then stays at `to`
    pub fn linear(at_0: f64, at_over: f64, over: Duration) -> Self; // straight line that continues after `over`
    pub fn sine(mean: f64, amp: f64, period: Duration) -> Self;
    pub fn noise(mean: f64, sd: f64) -> Self;                      // deterministic generator (seed)
    pub fn points(p: &[(Duration, f64)]) -> Self;                  // steps at the given instants
    pub fn replay(path: &str) -> Self;                             // .parquet or .jktrace
    pub fn every(self, d: Duration) -> Self;                       // new sample every d (default: every tick)
    pub fn with_quality_at(self, t: Duration, q: Quality) -> Self; // from t on
    pub fn with_quality(self, from: Duration, to: Duration, q: Quality) -> Self;
    pub fn frozen_after(self, t: Duration) -> Self;                // bit-for-bit equal value
}
```

The prelude of `joker-test` re-exports `QualityReason::*`, and so `Quality::bad(CommFailure)` works without a prefix.

### 6.6 Simulated devices

`SimDevice` is a simulated class instance for the harness, with the class simulator (14 A13; ref/03 §2.4.11) when there is one:

```rust
impl Sim {
    pub fn of<C: ClassDecl>() -> SimDevice;              // any class, members at Default
    pub fn temp_sensor() -> SimDevice;                   // one per std class with a simulator (generated)
    pub fn button() -> SimDevice;
    pub fn pump() -> SimDevice;
}
impl SimDevice {
    pub fn member(self, name: &str, s: impl Into<Series>) -> Self;
    pub fn constant(self, v: impl Into<Value>) -> Self;  // the main member of the class
    pub fn press_at(self, at: Duration, hold: Duration) -> Self;     // button
    pub fn fault(self, member: &str, q: Quality, at: Duration, for_: Duration) -> Self;
    pub fn capabilities(self, caps: &[&str]) -> Self;
    pub fn name(self, n: &str) -> Self;
}
```

### 6.7 Scenarios and class conformance

- `Harness::scenario(path)` loads a scenario from `sim/scenarios/*.scenario.toml` (ref/03) against the app alone: the `[[set]]`, `[[press]]` and `[[inject]]` actions are applied, and the `[[expect]]` expectations are checked by `h.check() -> ScenarioReport` (`passed() -> bool`, `failures() -> &[Failure]`).
- `joker_test::conformance::<C>(path, harness)` runs the cases `classes/<package>/<name>.conformance.toml` against an app that implements `C`; it is what `jk class verify` uses for a virtual instance.
- The expected values are written by hand, and never from an earlier execution (the rule of document 16, §8).

---

## 7. Runtime errors `J-E…`

Range of this document: `J-E0001`–`J-E0799` and `J-E0900`–`J-E0999`. All of them enter the `codes.toml` registry (16 §9). The traps (`J-E09xx`) count toward the crashloop (01 §9: 3 in 10 s); the others are values of `error.code`.

| Code | Severity | Message | `ActionErrorKind` / note |
|---|---|---|---|
| `J-E0001` | error | component world is newer than this runtime | refused before the deploy and at load |
| `J-E0002` | error | component world or cycle ABI is outside the supported window | §2.8, §3.1 |
| `J-E0003` | error-in-prod | component uses an unstable feature | §2.9 |
| `J-E0004` | error | joker:meta missing or invalid | §5 |
| `J-E0005` | error | joker:meta world does not match the component imports | |
| `J-E0006` | error | component imports an interface that is not allowed | §2.7 |
| `J-E0007` | error | app name in joker:meta does not match the manifest | also `jk check` |
| `J-E0008` | error | init-context rejected by the SDK (layout mismatch) | §2.6.2 |
| `J-E0009` | error | component is not a valid Wasm component or failed to compile | ADR 0052 |
| `J-E0010` | error | no usable AOT artifact for this engine and component | `prod` without `.cwasm`; ADR 0052 |
| `J-E0011` | error | app refused the new parameters | §4.3.4 |
| `J-E0050` | error | the app returned an error | `Error::app` |
| `J-E0101` | error | value unavailable: quality is not Good | `get()` |
| `J-E0102` | error | value unavailable: quality is Bad | `get_uncertain()` |
| `J-E0103` | error | port is not bound | `optional` port |
| `J-E0104` | error | value type does not match the port | tier 2 |
| `J-E0105` | error | record does not decode against its schema | |
| `J-E0203` | error | proposal buffer full; later proposals dropped | §3.4 |
| `J-E0204` | error | write was not applied | `require_applied()` |
| `J-E0205` | error | proposal-only writer cannot write directly | M18; `J0212` in `jk check` |
| `J-E0206` | error | write canceled: the generation changed before the commit | |
| `J-E0207` | error | scheduled-write queue full | 1.1 |
| `J-E0209` | error | this writer cannot create intents | 1.1 |
| `J-E0210` | error | intent TTL above max_manual | 1.1 |
| `J-E0301` | error | action denied by its permissive | `Denied` |
| `J-E0302` | error | action not allowed for this caller's role | `Forbidden` |
| `J-E0303` | error | action timed out | `Timeout` |
| `J-E0304` | error | device reported an error | `Device` |
| `J-E0305` | error | invalid action arguments | `Invalid` |
| `J-E0306` | error | action queue of the device is full | `Busy` |
| `J-E0307` | error | device not connected | `NotConnected` |
| `J-E0308` | error | action canceled | `Cancelled` |
| `J-E0309` | error | invalid idempotency key | `Invalid` |
| `J-E0310` | error | idempotency key reused with different arguments | `Conflict` |
| `J-E0311` | error | the instance does not support this action | `Unsupported` |
| `J-E0312` | error | the action implementation trapped | `Failed` |
| `J-E0313` | error | not a long action | `Invalid` |
| `J-E0401` | error | invalid history range | |
| `J-E0402` | error | history result too large | |
| `J-E0403` | error | signal is not recorded | |
| `J-E0404` | error | kv quota exceeded | |
| `J-E0405` | error | invalid kv key | |
| `J-E0406` | error | kv value too large | |
| `J-E0407` | error | kv storage error | |
| `J-E0408` | error | secret not declared or not bound | |
| `J-E0409` | error | secret not found on this node | |
| `J-E0410` | error | event payload too large or invalid | |
| `J-E0411` | warning | event dropped for a full consumer | counted |
| `J-E0412` | warning | mailbox full or message too large | counted |
| `J-E0413` | warning | network destination not allowed | host log |
| `J-E0414` | error | too many ports in one request | |
| `J-E0501` | error | instance health is Failed | `read()?` |
| `J-E0502` | error | the instance lacks this capability | |
| `J-E0503` | error | no instance with this name in the Many | `by_name` |
| `J-E0504` | error | invalid method arguments | decision log |
| `J-E0505` | error | method not in the class contract | tier 2 |
| `J-E0506` | error | member not bound on this instance | |
| `J-E0507` | error | write to a member of a virtual instance: the implementation owns it | decision log; 16 P6 |
| `J-E0601` | error | feature not available in this runtime | stubs of §2.9 |
| `J-E0602` | error | inference deadline exceeded | 1.1 |
| `J-E0603` | error | inference backend error | 1.1 |
| `J-E0604` | error | tensor shape or dtype mismatch | 1.1 |
| `J-E0605` | error | tensor too large for the Wasm boundary (1 MiB) | 1.1 |
| `J-E0606` | error | frame expired | 1.1 |
| `J-E0607` | warning | blocks lost (slow consumer) | 1.1 |
| `J-E0701`…`J-E0721` | error | compilation errors | §4.16 |
| `J-E0901` | trap | async API called from the cycle instance | `async-io`, `kv`, `secrets`, `ml`, `frames`, `blocks` |
| `J-E0902` | trap | cycle API called from the async instance | `cycle-io` |
| `J-E0903` | trap | heap allocation during a cycle task | §4.13; the SDK writes `J-E0903: …` to the log before the trap (ADR 0056) |
| `J-E0904` | trap | cycle task deadline exceeded | epoch (01 §4) |
| `J-E0905` | trap | guest panicked or called exit | |
| `J-E0906` | trap | memory limit exceeded | `memory.max` |
| `J-E0907` | trap | malformed proposal buffer or invalid port use | §3.4 |
| `J-E0908` | error | app is in crashloop | alarm `joker.app.<app>.crashloop` |
| `J-E0909` | error | init failed or timed out | |
| `J-E0910` | trap | stack overflow | |
| `J-E0911` | trap | fuel exhausted | only tests with fuel |
| `J-E0914` | error | migrate failed; previous version kept | |
| `J-E0915` | error | exported state too large | 1 MiB |

### 7.1 Action errors in the API

An action invoked through the API (`POST /v1/actions/{target}/{action}`, ref/09 §5.10) goes through the same path as an `invoke` of an app. The error result that the app receives and the one the API returns are the same fact with two codes:

| `invoke` (app) | `ActionErrorKind` | API: form | API: code | HTTP |
|---|---|---|---|---|
| `J-E0301` permissive false | `Denied` | request error | `J-E1402` | 409 |
| `J-E0302` role | `Forbidden` | request error | `J-E1002` | 403 |
| `J-E0303` timeout | `Timeout` | `Invocation` `failed`, `error.kind = timeout` | `J-E1406` | 200 |
| `J-E0304` equipment error | `Device` | `failed`, `kind = device` | `J-E1407` | 200 |
| `J-E0305` arguments | `Invalid` | request error | `J-E1401` | 400 |
| `J-E0306` queue full | `Busy` | request error | `J-E1403` | 503 |
| `J-E0307` device disconnected | `NotConnected` | `failed`, `kind = device` | `J-E1415` | 200 |
| `J-E0308` canceled | `Cancelled` | `failed`, `kind = cancelled` | `J-E1409` | 200 |
| `J-E0309` invalid key | `Invalid` | request error | `J-E1025` | 400 |
| `J-E0310` key repeated with other arguments | `Conflict` | request error | `J-E1024` | 422 |
| `J-E0311` the instance does not have the action | `Unsupported` | request error | `J-E1400` | 404 |
| `J-E0312` the implementation trapped | `Failed` | `failed`, `kind = device` | `J-E1408` | 200 |
| `J-E0313` not a long action | `Invalid` | request error | `J-E1405` | 409 |

An `Err` returned by an `on_action` reaches the API as `failed` with the code that the app chose, and `kind` derived from it by this table (`device` for a code outside it).

---

## 8. Expected behavior

1. **One call per task and per tick.** The host calls `on-cycle` in the order of the group (`order`, 01 §4 step 4); step 4b (virtual commands) runs on the returned records right after the task that produced them.
2. **Single writer per app** (16, A1): the proposals of the two instances count as the same writer and the same level; between them, A4 applies.
3. **Read:** the cycle instance always reads from the latch (02 §6.1), and an output shows the value applied in the previous tick. The async one reads the current slot (02 §6.4).
4. **Quality:** the table of 16 §3.1 applies as it is to `get`, `get_uncertain`, `get_or` and `sample`, in both views and in the class handles. No API of the SDK converts a `Bad` sample into a number without `Result` (invariant 4).
5. **Async writes:** `set` and `write` enter the inbox of the owning group and are applied in its next tick (01 §5). The `WriteTicket` resolves after that commit with the final result; meanwhile, `last_outcome()` gives `Deferred`.
6. **Methods:** the expansion belongs to the core (17 §4.1). In the tick of the call, `pending()` of the expanded `out` members gives `None`; the result is seen in the next tick with `last_outcome()`, or at the `await` in async.
7. **Actions:** never in the cycle; serialized per device; the key deduplicates within `dedup_window` (16 E9); the manifest wins over the class in the configuration (17 §4.2).
8. **Traps:** they discard the proposals of that tick of that app; the leases keep running (01 §9). An `Err` is not a trap.
9. **Parameters:** they change at a tick boundary, the same for all the tasks of that tick.
10. **Determinism:** what the cycle instance sees (image, `now`, `random`, virtualized WASI) is a function of the trace and of the generation; with canonicalized NaNs and without relaxed SIMD (09, attack 10), the same `.wasm` gives the same proposal bytes on aarch64 and x86_64.
11. **Limits** (ref/00 §10 and those of this reference): ports per app 65,535; proposal buffer 16 KiB (max. 1 MiB); `args` 4 KiB; event 8 KiB; mailbox message 256 B; `snapshot`/`subscribe` 256 ports; `history` 10,000 points; kv as in §2.5.1.
12. **Compatibility:** §2.8 and §2.9. A 1.y app runs on every 1.x with x ≥ y, with the same proposals for the same inputs.

---

## 9. Coverage of the examples

The 39 `rust` blocks of `docs/**/*.md` outside `docs/ref/`, extracted by script. "✓" means that everything the block uses is in this reference (or in ref/08, for the MCU apps, and in the `std/` classes of document 14); "error" points to the fix that the example needed. Rows 1–23 are from the documents and examples, as they were when this reference was written, and the fixes were applied in the integration of the API round (document 22, §7). Rows 24–39 are from the rewritten track (eleven parts, document 23), checked by a second script that looks up each attribute, method and type of the block in this reference and in ref/08; the only names that are not here are class members (`pressed`, `blink`, `alert`, `LightMode::*`), which come from the code generated from the `.jkc`.

| # | Block | Uses | Status |
|---|---|---|---|
| 1 | 00 §"DX" (l. 74) | `#[joker::app]` without arguments, `#[input/output/param(unit, default)]`, `#[joker::cycle(period)]`, `get()?`, `set` | ✓ |
| 2 | 04 §3.1 | `app(name, version)`, `unit`/`quantity`/`range`/`default`, `#[internal] Internal<Plan>`, `#[model]`, `#[alarm] Alarm`, `#[output(event)] EventOutput`, `derive(Record)`, `UtcSeconds` | ✓; `#[model]` is 1.1 (`unstable-ml`); **error**: `LowSoc` is not defined (`#[derive(joker::Event)] pub struct LowSoc { soc: f64 }` is missing) |
| 3 | 04 §3.2 | `get()?`, `get_or_default()`, `now().utc_seconds()`, `raise/clear`, `dt()`, `set` | ✓ |
| 4 | 04 §3.3 | `task(every, jitter)`, `history(…)`, `infer`, `Tensor::from_features`, `log::warn!`, `task(on)`, `Sample<f64>`, `emit` | **error**: `15.min()` → `15.mins()`; **error**: `io.calendar_features(now)` does not exist (a function of the app: `calendar_features(now)`); `infer`/`Tensor` are 1.1 |
| 5 | 04 §3.5 | `#[input(event = "Detections")]`, `#[action] Action<(), ()>`, `task(on_event)`, `write_at` | **error**: `event` is a flag (`#[input(event)]`), the type comes from the generic; **error**: `Class::Defect` is not defined; `write_at` is 1.1 |
| 6 | 04 §3.6 | `joker::classes::std`, `#[device]`, `requires`, `Many` | **error**: `Many<Pump, 1..=8>` → `Many<Pump, 1, 8>` |
| 7 | 04 §3.7 | `Harness`, `group`, `param`, `input`, `Series::ramp/linear/constant`, `internal`, `run_for`, `output`, `input_series`, `when`, `lt`, `all`, `traps`, `with_quality_at`, `Quality::bad(CommFailure)`, `after`, `no_new_writes` | **error**: `10.min()` → `10.mins()` (three times) |
| 8 | 06 §4 | trait `InferenceBackend` | out of scope (06, internal) |
| 9 | 06 §6 | `infer`, `y.quality.code` | 1.1; **error**: `Good =>`, `Uncertain =>`, `Bad =>` are binding patterns that catch everything: `QualityCode::Good` etc. |
| 10 | 14 §6.1 | `#[device] Led, Button`, `clicked()?`, `toggle()` | ✓ |
| 11 | 14 §6.2 | `Many`, `requires`, `Tank`, `Meter` | **error**: `Many<…, 1..=8>`; **error**: `Meter` → `MeterAc3` (or `use … MeterAc3 as Meter`); `tank@1` is not in `std/` (14 §10) |
| 12 | 14 §6.3 | `get("p1").read()?`, `PumpState`, `Health`, `Error::quality` | **error**: `get` returns `Option`: `io.pumps.by_name("p1")?.read()?` |
| 13 | 14 §6.5 | `derive(joker::Class)`, `Many<Zone, 1..=64>` | **error**: `Many<Zone, 1, 64>` |
| 14 | 14 §6.7 | extension trait, `Instant`, `500.ms()`, `toggle()` | **error**: `impl LedExt for Led<'_>` → `for LedCycle<'_>` |
| 15 | 14 §7.1 | `#[joker::implements(Pump, capabilities)]`, `cycle(init = …)`, `io.cmd`, `io.state` | **error**: `io.state.speed` belongs to the capability `variable_speed`, which `pump_ctl` does not offer (only `hour_counter`): it does not compile |
| 16 | 14 §9.1 | `Many`, `healthy()`, `index()`, `iter_mut()`, `len()`, `InputMany`, members as getters | **error**: `Many<…, 1..=8>`/`0..=16`; **error**: `io.pumps.len()` inside the `for … in io.pumps.iter_mut()` is E0502 (use `iter()`, the methods are `&self`, or compute `len` before); `FlowMeter`, `Tank` are not in `std/` |
| 17 | 14 §9.2 | `Many`, `iter().filter`, `served_by()`, `ahu.supply_sp()?`, `set_supply_sp().await?` | **error**: `Many<…>`; `Ahu`/`Vav` → `HvacAhu`/`HvacVav`; **error**: `v.served_by() == ahu.name()` compares `Result` with `&str` (`v.served_by().is_ok_and(|s| s == ahu.name())`) |
| 18 | 14 §13 | `#[joker::method(writes = …)]` | future alternative, not API (§11) |
| 19 | 19 §5 | `Input<f32>`, `Param<f32>`, native backend | ✓ |
| 20 | example 00 | `range`, `unit = "1"`, `wcet` | ✓ |
| 21 | example 01 (curtail) | `#[output(unit, many)] OutputMany`, `iter_mut`, `set` | ✓ |
| 22 | example 01 (Plan) | `derive(Record)`, `utc_seconds() + 30 * 60`, `Tensor::as_slice` | ✓; `Tensor` is 1.1 |
| 23 | example 02 | `derive(Event)`, `MonoNanos`, `BoundedVec`, `Quality` in records, `#[input(event)]`, `#[publish] Counter`, `inc()`, `write_at` | ✓; `write_at` is 1.1; **error**: `Class` is not defined (`#[derive(joker::Enum)] pub enum Class { Ok = 0, Defect = 1 }`) |
| 24 | track 1 §5 | `app(name, version)`, `#[output]`, `#[param(unit, range, default)]`, `cycle(period, wcet)`, `dt()`, `set`, `Default` state | ✓ |
| 25 | track 1 §9 (tests) | `group`, `param`, `run_for`, `output`, `toggles`, `toggles_every`, `safe`, `stop_app`, `ms()`/`s()` | ✓ |
| 26 | track 3 §3 | `#[input(unit)]` in `°C`, `#[param(unit, range, default)]` in `°C` and `ms`, `get()?` (`Err` without a `good` temperature) | ✓ |
| 27 | track 3 §7 (tests) | `input`, `Series::constant`, `with_quality`, `Quality::bad`, `after`, `between`, `errors`, `toggles_every` | ✓ |
| 28 | track 4 §7 | `cycle(init = …)`, `Input<bool>`, `Default::default` | ✓ |
| 29 | track 5 §3 | `#[joker::state(persist)]`, `get_or`, `#[param(unit, default)]` | ✓ |
| 30 | track 5 §7 | `sample()`, `quality.is_good()` | ✓ |
| 31 | track 7 §6 | `joker::classes::std`, `#[device]` (`Led`, `Button`, `TempSensor`, `AnalogIn`), `clicked()`, `pressed()`, `temperature()`, `raw()`, `off()`, `set`, `get_or` | ✓; the members are those of the `std/` classes (document 14) |
| 32 | track 8 §4 | `#[joker::implements(StatusLight)]`, `#[device] Led`, the `enum LightMode` generated from the `.jkc`, `Option` in the state | ✓; `LightMode` and the members come from the code generated from the `local/` class (§4.8.4) |
| 33 | track 8 §4 (tests) | `group`, `device`, `command`, `member`, `after`, `toggles_every`, `Sim::of` | ✓ |
| 34 | track 8 §6 | `#[device] indicator: StatusLight`, methods `blink`/`off`, `clicked()`, `pressed()`, `temperature()`, `raw()` | ✓ |
| 35 | track 10 §2 | native MCU app: `app`, `#[input]`, `#[output]`, `#[param(unit, range, default)]`, `cycle(period)` without `wcet`, `dt().as_secs_f32()` | ✓ (ref/08 §2.11; `as_secs_f32` is from `core::time::Duration`) |
| 36 | track 10 §4 | `#[joker::task(every)]`, `history`, `now()`, `h()`, `mins()` on an MCU target | error on purpose: `J0720` (`history` does not exist on Joker Micro, ref/08 §2.11.5) |
| 37 | track 10 §4 (line) | `#[param(unit, range, default)]` with `Param<f64>` on an MCU with `fpu = "sp"` | warning on purpose: `J0721` |
| 38 | track 11 §4 | `#[input] Input<bool>` bound to `alarm.<a>.active`, `get_or`, method `alert()` of the class | ✓ |
| 39 | track 11 §5 | pure Rust function (`Vec<f64>`) | ✓; 0.7.1 compiles and passes the tests: the defect is one of timing (overruns on the Pi Zero W), and not of API |

Mentions outside blocks: `#[joker::state(persist)]` (§4.12), `Mailbox<T, N>` (§4.2), `#[publish] Published<T>`/`Counter` (§4.2), `InputMany`/`OutputMany` (§4.6), `WriteTicket` (§4.6), `ActionHandle` (§4.9), `raw()` (§4.8.2), `x.mine()`/`x.winner()` (§4.6). **Error:** the `#[joker::shared]` of document 04 §3.4 does not exist and the promised error cannot be implemented (one mutable `static` cannot be told apart from another); the separation of the memories is guaranteed by the two instances, and it is documented. **Error:** the output of `jk inspect` in the old track (part 6, §5: "methods used", "queries used") cannot be obtained without static analysis; `joker:meta` lists the methods and members **known** to the handle.

---

## 10. Stress

**S1 — `Many<Pump, 1..=8>`.** Documents 04 and 14 write it like this in seven places. A const generic argument that is neither a literal nor a block must go between braces, and the error comes from the rustc parser, before `#[joker::app]` receives the tokens. *Broke.* **Fix:** `Many<C, MIN, MAX>` with two `usize` (§4.8.3), which is stable Rust, gives the capacity of `read_all()` at compile time and is what `jk check` compares (`J0610`).

**S2 — `15.min()`.** With `DurationExt` for `u64` in the prelude, `15.min()` has two candidates (`Ord::min` and `DurationExt::min`), and that is error E0034; on `f64`, the inherent method `f64::min` wins and asks for an argument. *Broke.* **Fix:** `mins()` (§4.7). `ms()`, `s()`, `h()` and `d()` do not collide with anything.

**S3 — A `Vec` in the cycle state.** With the bump allocator reset at each tick (document 04, §3.2), `st.buf.push(x)` in one tick kept a pointer to memory that the next tick reused: silent and nondeterministic corruption. *Broke.* **Fix:** in `on-cycle`, the allocator traps (`J-E0903`), and the harness shows the tick and the stack; `init` may allocate; the cycle API only uses bounded types (§4.13).

**S4 — `on-cycle` with `Err`.** The signature of document 04 (`result<list<u8>, string>`) lost the buffer when the task returned `Err`, against the rule of keeping the proposals already made. *Broke.* **Fix:** `cycle-result { proposals, error: option<error> }` (§2.6.1).

**S5 — `write(v).await?` does not mean "written".** `Rejected(Interlock)` is a result, and not a transport error: `?` lets it through. It is the most likely usage error of the async API. **Fix incorporated:** `Err` only when nothing was proposed (`J-E0205`, `J-E0206`); `WriteOutcome::require_applied()?` for whoever wants to treat non-application as an error (`J-E0204`); the example of the SDK documentation uses it.

**S6 — A 1.0 app on a 1.2 runtime with a new quality reason.** Through WIT, the adapter did not let the new case through; through the packed image, the new byte reached the app without any type preventing it, and the 1.0 SDK would not know what to do. *Broke.* **Fix:** each new case has a declared projection, and the host projects **before** writing the image and the WIT values, according to the world of the app (§2.8). The Rust `enum`s are `#[non_exhaustive]` so that the source code also survives an update of the SDK.

**S7 — The ML reservations locked into 1.0.** Document 11 puts `ml` and `frames` in the v1 world. If they were part of the stable 1.0 world, their shape would be fixed for the whole 1 series (golden rule), before the ML subsystem exists. *Broke.* **Fix:** `@unstable(feature = …)` in the package itself, outside the stable world; refused in `prod` (`J-E0003`), stubs in `dev`; stabilized in 1.1 with `@since` (§2.9). Scheduled writes (M6) and intents (M18), which document 11 puts in 1.1, get the same treatment.

**S8 — A malicious proposal buffer.** A guest may return a false `n_records`, a method `len` that goes beyond the buffer, an input port in a `WRITE`, or reserved flags. **Holds:** complete structural validation before applying any record, and the whole buffer discarded with a trap (`J-E0907`), so that there is never a partial application of an atomic output; value errors (NaN, nonexistent enum) are `rejected(wrong-type)` only on that proposal (§3.4).

**S9 — `HashMap` in a Rust app.** `std` asks for `wasi:random/insecure-seed`, and the component does not link if the host does not provide it, or stops being deterministic if the host provides it with real entropy. *Broke* (document 04 did not talk about WASI). **Fix:** the host provides the WASI set of §2.7, always virtualized: randomness from the deterministic generator, clocks from the tick, no files, network only in async and with `net.allow`.

**S10 — A `pump@1.2` class in the app, a `pump@1.0` instance on the site.** If the member indices came from the contract at runtime, member 5 of the app would be something else on the instance. **Holds:** the port table is built from the member list **of the handle**, recorded in `joker:meta`, and the member that the instance does not have gets `bound = false` (§2.6.2).

**S11 — `h.output("led").last() == Some(false)` without types.** A generic trace `OutputTrace<T>` would require an annotation in the tests that only count `toggles()`, and a trace of `Value` cannot be compared with `Option<bool>`. *Broke* with the first form. **Fix:** traces of `Value`, with `Value: PartialEq<f64 | bool | i64 | u64>` and `Observed: PartialEq<Option<T>>` (§6.4): the tests of the track compile as they are.

**S12 — Performance of an app with 120 VAVs.** 720 ports, of which ~240 outputs: image of 64 + 24 × (720 + 240) ≈ 23 KB per tick, one copy through the canonical ABI (~3 µs on an A76 at 8 GB/s effective, to be measured), and no call to the host. The result per output costs 24 B, and it is what allows `last_outcome()` on 240 outputs without 240 calls. **Holds**, within the budget of document 09, attack 2.

---

## 11. Decisions and rejected alternatives

| Decision | Rejected alternative | Why |
|---|---|---|
| 1.1 reservations with `@unstable` in the same package | separate package `joker:ml` with its own version | it would break the rule "one version for the world" of ref/00 §1 and force a version matrix; `@unstable` is WIT's own mechanism and leaves the stable world without changing anything |
| `cycle-result` with proposals and error | `result<list<u8>, error>` | it loses the proposals of a task with `Err` |
| no allocation in `on-cycle` (trap) | bump allocator reset per tick | dangling pointers in the state, without error (S3) |
| port = position in the table built in the order of `joker:meta` | opaque handles given by the host | the table can be rebuilt by the SDK and by `jk inspect`, and the member indices do not depend on the contract at runtime (S10) |
| WIT enums with a declared projection | `u8` codes in WIT | ref/00 fixes the WIT enums; the projection gives tolerance without losing the types |
| records with integer keys | text keys | a 3-field `Internal<T>` would hardly fit in the 64 B with names; the schema is always available in `joker:meta` |
| `Many<C, MIN, MAX>` | `Many<C, {1..=8}>` | a `RangeInclusive` as a const parameter is not stable, and the macro would have to rewrite it into a type that the user does not see |
| `mins()` | `min()`, `minutes()` | `min` collides; `minutes` is out of line with the other short suffixes |
| registration of the tasks at link time | `joker::export!(App, [control, replan])` | no example has it, and forgetting a task in the list would be a silent error |
| `joker:meta` as a sequence of fragments | one macro that knows everything | an attribute macro does not see the other items of the crate |
| errors with `record error { code, message }` where the app decides | variants per interface (`action-error`) | a registry code is extensible without changing the WIT, and it is what `jk explain` and the API use; the `cancelled` of long actions is `J-E0308`, without a new case in the WIT |
| one `run-task` per firing, also in `on` and `on_event` | one long `run-task` that consumes `subscribe` in the guest (proposal of ref/05) | the host counts and merges the firings (§4.3.2) in the same way in all languages, and a guest that ends does not leave a silent trigger |
| plan B as a sibling world `app-blocking` (§2.11) | blocking functions in the `app` world | the `app` world stays the same in the three languages; plan B only exists if an ADR activates it |
| harness traces over `Value` | generic traces | S11 |
| methods as pure Wasm functions (14 §13) | — | it remains a future alternative; nothing in this API prevents it |

---

## 12. Version markers

| Item | Phase |
|---|---|
| `types`, `cycle-io` (except `write-at`), `async-io` (except `write-at` and `propose`), `kv`, `secrets`, `log`, `guest` | 1.0 |
| cycle ABI v1 (§3), except the `AT` flag | 1.0 |
| the whole SDK of §4, except §4.15 | 1.0 (backends `wasm` and `native-linux` in Phase 1; `native-micro` in Phase 2, document 11) |
| virtual instances, `#[implements]`, `on-action`, `on_action` | 1.0 (Phase 2) |
| `joker-test`, except `Series::replay` of `.jktrace` with ML | 1.0 (Phase 0 for the harness; Phase 1 for the component) |
| `ml`, `frames`, `blocks`, `write-at`, `propose`, `AT` flag, `#[model]`, `Tensor` | **1.1**, reserved with `@unstable` |
| app shadow (M16): `run-mode.shadow`, the `SHADOW` flag of the cycle ABI and the `shadowed` outcome | **1.1** (document 26, §5; ADR 0035). They are cases of enums and bits that the 1.0 world already carries, so they are not gated with `@unstable`: a 1.0 host never produces them, and the SDK keeps them so that an app built against 1.0 runs unchanged under a 1.1 shadow |
| world `app-blocking` (tier 2 plan B, §2.11) | reserved; only with an ADR in Phase 3 |
| Wasm on Joker Micro (`wasmi`) | 1.4 (document 21): the cycle ABI does not change |
| WIT world 2 | 2.0 (ADR 0022) |
