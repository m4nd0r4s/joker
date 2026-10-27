# 01 — Architecture

Normative document. It describes the final design, with the primitives M1–M23 that came out of documents 08 and 09 already incorporated. Where a section exists because of a stress round, the reference appears in parentheses.

The core semantics is in document 16 and the common representation in `ref/00`, which take precedence over this document. The interfaces (manifest, lock, WIT, drivers, Joker Link, C ABI, API, agent) have exhaustive references in `docs/ref/` (ADR 0023); this document holds the explanation, with cross-references.

---

## 1. Layered view

```
┌──────────────────────────────────────────────────────────────────────────┐
│  APPS (Wasm components)           Rust · TypeScript · Python             │
│  typed ports ─ cycle (sync) ─ tasks (async) ─ actions ─ kv               │
├──────────────────────────────────────────────────────────────────────────┤
│  WASM HOST (joker-host)   wasmtime · AOT · pooling · epochs · capabilities│
├───────────────┬───────────────────────────────┬──────────────────────────┤
│  CORE         │  ML (joker-ml)                │  SERVICES                │
│  (joker-core) │  backends: burn · ort · hailo │  historian (joker-store) │
│  registry     │  · tensorrt                   │  uplink    (joker-link)  │
│  proc. image  │  .jkm models · pipelines      │  local API (joker-api)   │
│  arbiter      │  streams · training           │  bridges (ROS 2, Zenoh)  │
│  envelope     │                               │  OTA · config · secrets  │
│  expressions  │                               │                          │
│  alarms       │                               │                          │
│  trace        │                               │                          │
├───────────────┴───────────────────────────────┴──────────────────────────┤
│  HAL (joker-hal)   trait Driver · read plans · transports                 │
├──────────────────────────────────────────────────────────────────────────┤
│  DRIVERS  tier N (native) · tier W (Wasm) · tier X (external process)    │
│  modbus · opcua · gpio · i2c · spi · can/canopen · ethercat · revpi ·    │
│  m-bus · mqtt · serial · v4l2 · sim · …                                  │
├──────────────────────────────────────────────────────────────────────────┤
│  LINUX  PREEMPT_RT (recommended) · cgroups v2 · systemd · /dev/*          │
└──────────────────────────────────────────────────────────────────────────┘
```

Dependency rules:

- `joker-core` does not depend on any layer above or below it. It receives a `Clock`, receives samples and returns applied writes. It is what gets proven, property-tested and replayed.
- Drivers do not know apps; apps do not know drivers. The only meeting point is the **path of a signal**, resolved to a **dense handle** (`u32`) at the start of each configuration generation (§15).
- The services (historian, uplink, API) consume the core only through bounded lock-free queues. No service can stall the core (ADR 0013).

---

## 2. Processes and threads

Joker is **one process** (`jokerd`) plus optional auxiliary processes:

| Process | When it exists | Why |
|---|---|---|
| `jokerd` | always | core, Wasm host, tier N and W drivers, in-process ML, services |
| `joker-drv-<name>` | tier X driver | vendor SDKs (C blobs, licenses), fault isolation |
| `joker-ml-<id>` | model with `isolation = "process"` | GPU/NPU drivers that can hang or crash the process |

Inside `jokerd`:

| Thread | Class | Scheduling | Function |
|---|---|---|---|
| `rt/<group>` (one per cycle group) | `rt` | `SCHED_FIFO`, rate-monotonic priority (shorter period → higher priority), pinned to an `rt` CPU | latch, cycle tasks, arbiter, envelope, commit |
| `rt/supervisor` | `rt` | `SCHED_FIFO` max. | watchdog of the groups, lease expiry, trips, feeds the hardware watchdog |
| `rt/drv/<bus>` | `rt` | `SCHED_FIFO` | only for buses with their own cycle (EtherCAT, piControl with sync) |
| `io` (tokio multi-thread runtime) | `io` | `SCHED_OTHER`, nice −5 | async tier N drivers, transports |
| `app` (tokio runtime) | `bg`/declared | `SCHED_OTHER` | async tasks of the apps (WASI 0.3) |
| `ml/*` (pool) | `ml` | `SCHED_OTHER` / `SCHED_IDLE` for training | inference, pipelines, training |
| `svc` (tokio runtime) | `bg` | `SCHED_OTHER`, nice 5 | historian, uplink, API, OTA |

**Resource classes (M8).** `rt`, `io`, `ml` and `bg` are cpusets (cgroups v2) declared in the manifest. On a board with 4 CPU cores, the recommended configuration is `rt = [3]` with `isolcpus=3 nohz_full=3 rcu_nocbs=3` on the kernel, `io = [2]`, `ml = [1,2]` and `bg = [0]`. `jk doctor` checks this (document 07).

**Why it is a single process.** EdgeX pays latency and memory for each microservice, and Kura pays for the JVM. With one process, a read goes from the driver to the app with no serialization and no IPC: one 32-byte copy into the slot, another into the snapshot. Fault isolation, which is the argument for several processes, is obtained in another way. Apps are in a Wasm sandbox (a trap does not bring down the process), and code that is not trusted with native memory (vendor blobs, GPU drivers) goes into tier X processes.

---

## 3. The core: signal registry and process image

### 3.1 Registry

At the start of each configuration generation, the manifest and `joker.lock` are compiled into an **immutable registry**:

- each signal has a dense `SignalId(u32)`, ordered by path (which gives determinism);
- metadata: type, canonical unit, direction, owning device, `max_age`, envelope (if it is an output), owning group (if it is an output), recording policy;
- derived tables: signals per driver, ports per app, expression dependencies.

**No string lookup happens on the hot path.** Apps receive handles at initialization; expressions are compiled to bytecode over `SignalId`.

### 3.2 Last-value store

Each scalar signal has a **32-byte slot** aligned to the cache line (two per line):

```
struct Slot {            // 32 bytes
    version: AtomicU32,  // seqlock: odd = write in progress; sample seq = version / 2
    quality: u32,        // code (8 bits) + reason (8 bits) + flags (16 bits, includes the ts_src domain)
    value:   u64,        // value bits (bool/i64/u64/f64/f32 promoted; enum = u64)
    ts_src:  i64,        // ns: UTC of the remote device, or local hardware monotonic (flag); i64::MIN if none
    ts_acq:  u64,        // monotonic ns (CLOCK_MONOTONIC) of reception
}
```

**Timestamp domain.** The `ts_src` field holds one of three timestamps, and the flags of the quality word say which: the UTC of the remote device (`ts_src` proper), the monotonic capture time in local hardware (`ts_hw`), or the `ts_hw` of an MCU **converted** to this node's monotonic clock by the Joker Link `TIME` estimate ("link" domain, `ref/07` §8.3). A converted timestamp carries the uncertainty of the estimate, and so it is never confused with a local one. The exact bit layout is in `ref/00` §4.3 and in `ref/04` §3.2.

Each slot has **a single writer**: the acquisition stage of the owning driver (ingestion) or, for derived signals, the owning cycle group. Readers (group latch, async tasks, historian) use the seqlock protocol, with one rule: **a reader never waits for a writer**. If a read collides with a write in progress, the reader retries it a bounded number of times and, if it keeps colliding, uses the previous complete sample and counts the collision. On a single-core MCU, where the reader can be an interrupt that preempted the writer itself, it does not retry: it uses the previous one right away (`ref/08` §2.4.1). A reader that waited would be blocked forever. `string`, `bytes` and `record` values (≤ 64 B) live in a **wide slot**: the same seqlock and the same rules, with a payload of up to 64 B and its length (96 B; ADR 0062), in a separate table indexed densely among the wide signals. Larger values are streams (§11).

The 64-bit `seq` exposed to apps is the seqlock version divided by 2, extended by the reader with a per-signal wrap counter kept outside the slot (one wrap every 2³¹ samples). Target cost: write ~20 ns, read ~10 ns on a Cortex-A76.

### 3.3 Acquisition

```
driver ──RawSample──▶ driver SPSC queue ──▶ acquisition stage ──▶ slot
                                                 │ scale/offset/byte order
                                                 │ unit conversion
                                                 │ quality: link, physical limits, NaN
                                                 │ acquisition deadband
                                                 └▶ "changed" event (bitset per group)
```

The acquisition stage runs on the `io` thread, right after the driver, and it is core code: drivers deliver raw values plus the link state, and the semantics (scale, unit, quality) is applied in a single place.

**Aging.** Each signal has `max_age` (by default, 3 × the poll period). At the latch, a sample older than `max_age` is marked `Uncertain(Stale)`, and one older than `2 × max_age` becomes `Bad(Stale)`. The slot is not changed: this is computed at read time, because the age depends on when the read happens.

### 3.4 Per-group process image

Each cycle group has **its own process image**: a dense vector with only the signals that its tasks read (the bound inputs) and write (the owned outputs). It is preallocated at startup.

- **Latch**: at the start of the tick, the group copies the relevant slots into its snapshot. All tasks of the tick see exactly the same values.
- **Staging**: the writes of the tasks go into a proposal buffer of the group, and not into the slots.
- **Commit**: at the end of the tick, the proposals go through the arbiter and the envelope (§6, §7). The applied ones go to the output slots and to the output queues of the drivers.

**Output boxes for the drivers.** The commit does not write into a FIFO. Each output has a **latest-wins box** (the last applied value replaces the previous one not yet sent), so that slow or hung equipment never receives, when it wakes up, a queue of stale values. Only scheduled writes, pulses and atomic groups use an ordered queue, and that one is bounded. Each box also has an atomic `force_safe` bit that only the supervisor sets (document 09, attack 4): it is the emergency path when a group hangs, and it does not violate the single writer of the values.

Consistency between groups: a group reads the **applied** values (commits) of another group, in the state they were in at its latch. There is no transactional consistency between groups of different periods, and this is declared (document 02, §6.3). Whoever needs consistency between two tasks puts them in the same group.

---

## 4. The tick of a cycle group

```
 t_k = t_0 + k·T  (clock_nanosleep TIMER_ABSTIME, CLOCK_MONOTONIC)
 │
 ├─ 1. latch               copies N_in slots → snapshot           (~10 ns/signal)
 ├─ 2. async requests      drains inbox of async proposals → staging
 ├─ 3. expressions         derived signals, alarms and permissives of the group (bytecode)
 ├─ 4. tasks               for each task, in the declared order (`order` of the group):
 │                           synchronous Wasm call with epoch deadline
 │    4b. virtual commands   if a later task implements a virtual instance, the commands
 │                           of this task are arbitrated and go through the envelope right here (document 17, §5.3)
 ├─ 5a. methods            expand the method records of the proposals, in call order (document 17, §4.1)
 ├─ 5. arbiter             per output: picks the active lease with the highest priority (rules: document 16, §1)
 ├─ 6. envelope            per output: permissive → range → rate → deadband → atomic groups
 ├─ 7. commit              output slots + latest-wins boxes of the drivers + scheduled writes
 ├─ 8. trace               entry in the ring: tick seq, seqs read, write results
 └─ 9. statistics          jitter, duration, overruns (allocation-free histograms)
```

**Budget.** Each task declares or measures (`jk bench`) its WCET. `jk check` adds up the budgets per `rt` CPU and applies the rate-monotonic schedulability test (Liu & Layland, with a 30% margin, and the response-time analysis when the bound is not enough; the formula and what goes into the WCET are in ADR 0048). A manifest that does not pass is rejected (**real-time admission**, M23). This is what stops someone from putting a 1 ms group with 900 µs of tasks and discovering the problem in the field. **Admission does not guarantee the absence of overruns on a shared CPU core:** the WCET is measured with the task alone (`jk bench`), and with the `rt` CPU shared with `io`, `bg` or the system (a board with `cpus = 1`, or without `isolcpus`), the interrupts, the cache and the scheduler of other classes add to the real time; the 30% margin cushions, it does not prove, and overruns are always counted (`jk status`).

**Deadline and overrun.** Each Wasm call has an epoch deadline: the supervisor increments the engine epoch every 100 µs, and the instance is interrupted if it exceeds `max(wcet × 2, 200 µs)` (the epoch granularity does not allow finer deadlines; the WCET of short tasks is watched by measurement, not by interruption). If the whole tick exceeds the period, the group's policy applies:

| `overrun` | Behavior |
|---|---|
| `skip` (default) | the next tick is skipped to realign; counts `overruns` |
| `catch_up` | runs immediately, at most once; only for groups without time-dependent effects |
| `degrade` | disables the tasks marked `optional` until N clean ticks |
| `trip` | puts all outputs of the group in the safe state |

N consecutive overruns (10 by default) raise the alarm `joker.group.<g>.overrun` and, if `trip_after` is declared, the trip of the group.

**Native tasks.** For the rare cases where the Wasm boundary weighs (1 kHz with a lot of arithmetic), a cycle task can be written as a Rust crate compiled into `jokerd` (`kind = "native"`). The sandbox and hot swap are lost, and 1–3 µs per call are gained. The rule is Wasm by default and native with a justification.

---

## 5. The async domain

Async tasks (`#[joker::task]`) run on the tokio `app` runtime as WASI 0.3 `async` exports. They serve for events, slow timers, ML, heavy computation, authorized network and actions.

- **Read**: they see the *last value* (slot), not a snapshot, and each read carries `seq` and `ts`. To read several signals consistently, they ask for `snapshot([h1, h2, …])`, which repeats the seqlock read until it obtains a coherent epoch.
- **Write**: it is always a proposal that goes to the inbox of the output's **owning group** (SPSC per task→group pair). It is applied at that group's next tick and returns `Deferred`, with the final result arriving through a future (`WriteTicket`).
- **They never touch the cycle domain**: there is no synchronous call from the async domain to the cycle domain, nor the other way around.

**One app, two instances.** A wasmtime `Store` cannot be used by two threads at the same time. An app with cycle tasks and async tasks is therefore instantiated **twice** from the same component: a cycle instance (thread `rt/<group>`) and an async one (tokio `app`). They do not share linear memory. They communicate through the app's **internal signals** (`#[internal]`, which live in slots like the others) and through bounded **mailboxes**. The SDK makes this explicit in the type system (document 04, §3.4). The decision and the rejected alternatives are in document 09, attack 6.

---

## 6. The arbiter (M15)

Each output has an arbiter with **16 priority levels**, where 1 is the highest (like the BACnet *priority array*). Each authorized writer occupies a level, declared in the binding:

| Band | Name | Typical use |
|---|---|---|
| 1–2 | `safety` | protection apps (software, they never replace hardware protection) |
| 3–5 | `manual` | operator (CLI, API, HMI) |
| 6–8 | `supervisor` | site coordinator, curtailment ordered by the grid operator |
| 9–12 | `control` | regulation controllers |
| 13–15 | `optimizer` | optimizers, ML models |
| 16 | `schedule` | calendars, base values |

Semantics:

1. A write from a writer places `(value, lease_until)` at its level. Each write renews the lease.
2. The effective value is that of the **active level with the highest priority** (lease not expired).
3. `relinquish()` frees the level immediately.
4. **With no active level, the output goes to the safe state.** There is no configurable *relinquish default* value separate from `safe`: a "resting" value that was not safe would be a second truth.
5. `manual` writers have a **mandatory TTL on every write**, with no default; the maximum is the output's `max_manual` (default 8 h). The forgotten override is the most common problem in building management systems, and here it expires on its own. 15 minutes before the end, the alarm `joker.output.<s>.manual_expiring` (severity `medium`) and an event on the `manual` topic; with a TTL ≤ 15 min, only the event, because an immediate alarm would be noise (`ref/09` §5.6). The manual levels survive a restart of `jokerd`, with the deadline in UTC (16, A14); if the clock is not `synced` at startup, the remaining TTL is restored and the alarm `joker.output.<s>.manual_restored` (severity `medium`) is raised, so that an operator confirms or releases.
6. Each level only accepts the declared writer. Two apps do not share a level, and two bindings from different apps on the same output without distinct levels are a validation error (`J0210`). On a manual level, occupied at runtime by a person, a second principal is refused unless it asks for `takeover` (16, A13).
7. Proposal-only writers (M18, **1.1**) do not occupy a level: they create **intents** that only become a `manual` write after human approval (§6.1).

On an output of an MCU, the arbiter is the MCU's; the Linux node is a remote writer on delegated levels (16, A12).

The arbiter runs **before** the envelope. The envelope applies to whoever won, whoever it is, including `safety` and `manual`.

### 6.1 Intents and approval (M18, **1.1**)

Reserved for 1.1 (D5): `writer.kind = "proposal"` is `J0290` on a 1.0 runtime, and the intent endpoints respond empty. The design stays here. A writer with `writer.kind = "proposal"` (an LLM assistant, an optimizer under test, an untrusted external system) does not write: it calls `propose(output, value, justification, ttl)`. The intent stays pending in the local API, and a human with the `operator` role approves or rejects it (`jk intent approve <id>`, or an HMI). Once approved, it is executed as a `manual` write with the TTL of the intent, on behalf of whoever approved it. Everything goes into the decision log.

---

## 7. The envelope

Applied per output, at the commit of the owning group, in the following order. The first step that fails decides the result. The exact rules are in document 16, §2 (E1–E10).

| # | Step | Failure → |
|---|---|---|
| 0 | **clock**: the winning writer is an app with `requires_clock = "synced"` and the node clock is not `synced` (§18) | `Rejected(ClockUnsynced)`, and the output goes to `safe`; renews the lease (16, A3 and E1) |
| 1 | type and unit (already converted by the binding) | `Rejected(WrongType)`; never happens if `jk check` passed |
| 2 | **permissive** (`interlock`): boolean expression over signals; `Bad` counts as false | `Rejected(Interlock)`, and the output keeps the previous value or goes to `safe`, according to `on_interlock` |
| 3 | **quality of the dependencies** (`requires_good`): signals that must be `Good` | `Rejected(Quality)` after `grace`, and the output keeps the previous value or goes to `safe`, according to `on_interlock` (16, E4) |
| 4 | **range** (`range`) | `clamp` → `Clamped(Range)`; `reject` → `Rejected(Range)`; `trip` → `Tripped` |
| 5 | **rate** (`rate`, unit/s) relative to the last applied value | `Clamped(Rate)` (applies the maximum step) |
| 6 | **deadband**, **minimum interval** (`min_interval`) and **actuation budget** (`max_actuations = "20/h"`, for relays and contactors) | `Suppressed`: it is not an error, and it does not use the bus; the exhausted budget gives `Rejected(Budget)` and an alarm |
| 7 | **atomic groups** (M20): if the output belongs to an atomic group, all outputs of the group pass together or none passes | `Rejected(AtomicGroup)` |
| 8 | commit | `Accepted` |

`safe` is a value or `relinquish`, which hands control back to the equipment's own logic or to the previous system (document 03, §6.1). **The safe value is always admissible.** Going to `safe` is not blocked by permissives nor by range. The rate only applies if a `safe_ramp` is declared, because there are processes in which jumping to the safe value is dangerous: closing a valve suddenly causes water hammer.

**Trip.** An output in trip stays at `safe` until there is an explicit `reset` (a `manual` action or a `reset_when` condition). The trip persists across restarts of `jokerd`, because it is kept in `state/trips`, with fsync.

**Readback verification.** An output may declare `verify = { readback = "pcs.p_set_rb", tolerance = "1 kW", within = "2s" }`. If the value read does not confirm the one written, the output becomes `Uncertain(NotConfirmed)` and the alarm `joker.output.<s>.unconfirmed` fires. This is what catches the case "the equipment accepted the Modbus and ignored the value".

---

## 8. Three-layer safe state

| Layer | Who guarantees it | What it covers | What it does not cover |
|---|---|---|---|
| **App lease** | arbiter + `rt` supervisor | dead or hung app, slow model, network of an async task | dead `jokerd` |
| **jokerd supervisor** | `rt/supervisor` thread + hardware watchdog (`/dev/watchdog`, RevPi, IPC) + systemd `WatchdogSec` | hung group, deadlock, dead runtime → board restart | lost communication with the equipment |
| **Device fallback (M1)** | the equipment itself | everything above, including the cut cable | nothing: it is the last line |

**M1: the device fallback is declared and verified.** Profiles declare how the equipment protects itself (communication timeout register, fallback value). At startup, the driver reads those registers and, if the manifest allows it, writes them. Then it compares them with Joker's `safe`. The rules:

- `device_fallback.value` must be **as safe or safer** than Joker's `safe` (the meaning of "safer" is declared per output: `safer = "lower" | "higher" | "equal"`);
- the runtime **renews the write** on the equipment every `keepalive` (by default, 1/5 of the equipment timeout), even if the value does not change; deadband and `min_interval` never suppress a keepalive;
- `device_fallback.timeout` must be **greater** than `keepalive` plus the restart time of `jokerd` (measured by `jk doctor`), otherwise the equipment drops to fallback on every runtime update. The writers' leases do not enter this calculation: they are internal to Joker and, when they expire, the runtime actively writes `safe`;
- a restart of the whole **board** (hardware watchdog) takes longer than the timeout, and the equipment fallback is the right behavior in that case;
- a missing or incoherent fallback is a `warning` in `dev` and an **error** in `prod`, unless there is `accept_missing_fallback = "<justification>"`, which is kept in `joker.lock`.

---

## 9. Wasm host (joker-host)

- **Engine:** wasmtime, on its most recent long-term support line (48 in Phase 0; `policies/dependencies.md` §7), with WASI 0.3 and native async. Cranelift on x86_64, aarch64 and riscv64; the Pulley interpreter on armv7 and on other architectures without Cranelift (document 08, round 5).
- **AOT is mandatory in production.** Components are precompiled (`.cwasm`) at `jk build --target` or at installation on the device, with the hash of the engine and of the CPU flags (the engine identity, with the triple's baseline flags, is the one of ADR 0052). There is never JIT on the cycle path.
- **Packed image.** The fast path of the cycle is a single call per task: `on-cycle(task, image) -> cycle-result`. The app's input image goes packed (24 B per sample and 24 B per output result) and the proposals come back in a buffer of packed records (a write is 14 B: type, flags, `u32` port, `u64` value). The exact layout is in `ref/04` §3. The tier 2 SDKs (TypeScript, Python) use the **same** image and the same buffer; the `cycle-io.read/write` calls are kept only for dynamic accesses and for values that do not fit in `u64` (document 09, attack 2; `ref/05`).
- **Tier 2 in the cycle** (D15): allowed, with a minimum period of 100 ms in TypeScript and 500 ms in Python; admission uses the **maximum** measured by `jk bench`, and not the p99.9, because GC pauses make the percentile misleading.
- **Engine determinism:** relaxed SIMD disabled and NaNs canonicalized in the cycle instances, so that the same `.wasm` gives the same bits on aarch64 and x86_64 (document 09, attack 10).
- **32 bits:** on armv7, memories with explicit bounds checks and without the 4 GiB virtual reservations (document 09, attack 3).
- **Pooling allocator.** Linear memory and tables pre-reserved per instance, with `memory.max` declared by the app. The default depends on the tier: 16 MiB in the cycle and 64 MiB in async in Rust; 64/64 MiB in TypeScript; 128/128 MiB in Python (the SDK puts the suggestion in `joker:meta`, and the manifest wins: `ref/01` §5.12, `ref/05` §19). The pages of the cycle instance are pre-touched and pinned. Allocating in the cycle, beyond what was reserved in `init`, is a trap (`J-E0903`, `ref/04`).
- **Deadlines:** epoch interruption (§4). Fuel is optional and is only used in deterministic tests.
- **Capabilities.** An app only receives handles for the bound ports. By default it has no file system, no sockets and no wall clock (it uses `cx.now()`, which comes from the tick). Network for async tasks only with `net.allow = ["api.example.com:443"]`, and files only in `kv` (§9.1).
- **Failures.** A task that ends with an error (`cycle-result.error`) keeps the proposals it has already put in the buffer. A trap in a cycle task counts as a tick failure of that app: its proposals for the tick are discarded and the leases keep running. K traps in a window (by default 3 in 10 s) put the app in `crashloop`, with the instance discarded, re-creation with exponential backoff (1 s → 5 min) and an alarm. The outputs follow the lease and go to the safe state on their own.
- **Hot swap.** A new version is instantiated alongside, receives the state via `migrate(old_state) -> new_state` (optional) and comes in at a **tick boundary**: at tick k the old version runs, at k+1 the new one. If `init` fails, the old one continues.

### 9.1 Persistent state of apps

`kv`: small key-value storage (≤ 1 MiB per app), transactional, with configurable fsync. Only async tasks have access to it. The cycle does no I/O: if it needs persistent state, it publishes an internal signal and an async task saves it.

---

## 10. ML (v1.1; summary; detail in document 06)

ML is not part of v1 (ADR 0019). What v1 already reserves for it is in document 11, "Prepared for ML".

- A **model** is declared in the manifest like a device: `.jkm` artifact, tensor signature, backend with a fallback chain, deadline, resource class and isolation.
- Apps call `infer` (async) with small tensors, or consume the outputs of **native pipelines** (M5), which take streams directly to the accelerator without going through Wasm memory.
- Backends implement the `InferenceBackend` trait. **Burn** is the default and reference backend (CPU Flex/CubeCL, Vulkan, CUDA, Metal for development). ORT, HailoRT and TensorRT are plugins.
- **Input guard (M17):** the signature declares plausible ranges per input, and an out-of-distribution input returns `Uncertain(OutOfDistribution)` along with the result, which the app or the fallback controller handles.
- **Non-deterministic outputs (M22):** the outputs of models on GPU/NPU are recorded in the trace, and replay uses them instead of re-executing.

---

## 11. Streams and bulk data (M4)

Images, point clouds and audio blocks do not fit in 32-byte slots. A **stream** is:

- a preallocated **buffer pool** (memfd or dmabuf when the driver supports it: V4L2, libcamera), of fixed size per stream;
- **reference-counted frames**, with metadata (hardware capture ts when it exists, seq, format, dimensions);
- `latest` semantics (slow consumers drop frames, and this is counted) or `queue(n)` (with backpressure on the producer up to n and drops from there on).

A frame is never copied between the producer and native consumers. Wasm apps receive a `frame` **resource** and may ask for `view(roi, format)`, which copies only the requested region into linear memory. The normal path for ML is the native pipeline (document 06, §5).

**Block signals (M7).** For waveforms (vibration at 25.6 kS/s, current at 10 kS/s), the driver delivers **blocks**: `t0`, `period`, `n`, contiguous samples and per-block quality. They live in a ring per signal, and consumers read whole blocks. The historian stores them compressed, and the scalar derived signals (RMS, peak, FFT bands) are computed by pipelines or tasks.

---

## 12. Local historian (joker-store)

- **Per-series segments**, append-only, with 4 KiB blocks: Gorilla compression (delta-of-delta on times, XOR on floats) for scalars, and zstd for waveform blocks and events.
- Shared **WAL**, with batched fsync (by default every 1 s, `fsync = "batch" | "always" | "never"`). At most 1 s is lost in a power outage. Only trips, manual writes, rejections and changes of active level use `always`; the rest of the decision log goes in a batch (§19).
- **Tiered retention:** `raw` (e.g. 7 d), aggregates `1s`, `1m` and `15m` (min/max/mean/last/count/dominant quality), each with its own `keep`. Retention by size has priority over retention by time (`max_disk = "8 GiB"`). With the disk at 90%, the oldest `raw` is deleted first.
- **Query:** local API (`/v1/history`), `jk history`, export to Parquet. Parquet is written by `jk`, from the API's NDJSON: the node does not carry a Parquet writer (`ref/09` §5.5).
- **Store-and-forward:** each uplink has a persistent cursor over the historian, and resending reads from the historian instead of keeping a separate queue. A network outage of days costs no memory, only disk, and is bounded by retention.

Target: 200 k samples/s of ingestion on a CM4 with eMMC, with 1–2 bytes per compressed sample on typical process signals.

The on-disk layout, the checkpoints that bound the WAL, the aggregate rules (a `Bad` sample only counts; the dominant quality is the most frequent code, the worse one on a tie), the order of retention by size and the critical disk are in ADR 0081.

**The storage medium matters.** The board declares `storage = "sd" | "emmc" | "ssd"` (`ref/03` §2.3.1). `jk check` projects the write load of the manifest (historian, decision log, `kv`, continuous trace, and the number of fsyncs) and `jk doctor` measures it on the node; above what the medium can take, the warning `J0381` says how much and where it comes from. An SD card is the most fragile case, and it is the bench's (document 23).

---

## 13. Uplink (joker-link)

| Type | Use |
|---|---|
| `mqtt` | generic, configurable topics and JSON/CBOR |
| `mqtt-sparkplug` | Sparkplug B: NBIRTH/DBIRTH, metrics with alias, node state; the mapping of signals to metrics is automatic |
| `zenoh` | efficient pub/sub/query, interop with ROS 2 (`rmw_zenoh`), local meshes |
| `otlp` | metrics, logs and traces **of the runtime itself** for observability |
| `http` | webhook for events and alarms |

**Budgets (M12).** Each uplink may declare `budget = "100 MB/month"` and priority classes (`alarms > events > telemetry.high > telemetry.low > bulk`). A consumption estimator projects the month. When the projection exceeds the budget, the low classes switch to aggregates (1 s → 1 min → 15 min) before anything is discarded. Alarms are never degraded. The budget state is a signal (`joker.link.<u>.budget_used`).

**Delivery:** `at_least_once` with a sequence number per series (the destination deduplicates) for history, and `latest` for live values. Order is guaranteed per series, not across series.

---

## 14. Local API and bridges

- **Local API** (`joker-api`): HTTP/1.1 and HTTP/2 with JSON, WebSocket and SSE for subscriptions, and published OpenAPI. It listens on a Unix socket (`/run/joker/api.sock`) by default and, optionally, on TCP, always with mTLS. Roles: `viewer`, `operator` (manual writes, approval of intents, reset of trips), `admin` (config, OTA). The `jk` CLI is only a client of this API. The exhaustive reference (endpoints, errors, anti-replay, WebSocket) is in `ref/09` and in `spec/openapi/joker-api.yaml`.
- **Bridges (M11):** bidirectional exposure of signals to other ecosystems: ROS 2 via Zenoh (`rmw_zenoh`), OPC UA **server** (Joker publishes its address space for SCADA), Modbus **server** (for legacy PLCs that want to read from Joker). **A write that arrives through a bridge is a write from a declared writer**, with priority, lease and envelope. A bridge is not a shortcut.

---

## 15. Configuration: generations and transactional apply

A **generation** is the pair (resolved manifest, `joker.lock`) with the compiled registry. Changing configuration is changing generation. Each generation has a `gen` number, given at creation, and a content hash, which identifies it (that of the `joker.config.v1+cbor` descriptor, `ref/10` §2.6.2).

```
jk apply  ──▶ validate (J-codes) ──▶ plan diff ──▶ prepare ──▶ swap ──▶ confirmation window ──▶ confirmed
                                         │                       │              │
                                         │                       │              └─ expired → automatic rollback
                                         │                       └─ at a tick boundary of each affected group
                                         └─ what changes: drivers to restart, apps to reload,
                                            envelopes to swap (hot), new/removed signals
```

- **Minimal diff:** changing an envelope does not restart the driver, and changing the address of a device restarts only that driver. The signals that survive keep their `SignalId` when possible, and unaffected apps continue without interruption.
- **Confirmation (ADR 0015):** `jk apply --confirm 5m` applies and waits for a `jk confirm`. If it does not arrive (because the new config cut the network, for example), it returns on its own to the previous generation. It is the `commit confirmed` of routers applied to the edge. In `prod`, the window is **mandatory** when the plan changes the network, security, the uplinks or the **API itself** (listen endpoints, roles): these are the plan's `confirm_reasons` (`network`, `security`, `uplinks`, `api`, `ref/09` §5.13).
- **Rollback reactivates** (D9): going back does not create a new generation. The previous generation becomes the active one again with the same hash and the same `gen`, and the generation history is a history of **activations**, each with its origin (`origin`; `ref/09` §5.13, `ref/10`). An activated generation that is never confirmed becomes `rolled_back`; a `pending` one that was never activated becomes `discarded` (`ref/00` §6, `ref/09` §5.13).
- **Overlays (M13):** `extends = ["templates/joker.bess-v2.toml"]` with deep merge and explicit rules (document 03, §2). `jk diff` shows the effective manifest.

---

## 16. OTA: four artifact types

| Artifact | Unit | Swap | Rollback |
|---|---|---|---|
| **runtime** (`jokerd`) | signed binary | A/B slots in `/var/lib/joker/runtime/{a,b}`; systemd starts the active one; health test (N clean ticks, drivers connected) | automatic if health fails within `health_window` |
| **app** | signed `.wasm` component + `.cwasm` | hot swap at a tick boundary (§9) | previous version kept warm during `keep_previous` |
| **model** | signed `.jkm` | shadow mode → promotion (M16) | automatic demotion by KPI |
| **config** | manifest + lock | generation with confirmation (§15) | confirmation window |

A new runtime starts **with the current confirmed generation**: the runtime OTA never changes configuration at the same time (`ref/10` §3). During the restart, the equipment covers the gap with its fallback (M1) and the MCUs with their leases.

Joker **does not update the OS**. It integrates with RAUC, SWUpdate or Mender for that (document 12). Fleet management belongs to **Joker Deck** (document 20, ADR 0021): `jokerd` exposes an agent with desired state and reported state (ref/10), which the Deck, or another manager, uses.

---

## 17. Security

- **Device identity:** **P-256** X.509 certificate issued by the organization's CA, with the key in a TPM 2.0 or secure element when one exists, and in a protected file when it does not (this is declared in `jk doctor`). Enrollment also creates the node's X25519 key for secrets (ADR 0025, `ref/02` §2.5).
- **Signatures:** runtime, apps, drivers, profiles, boards, classes, firmware, production configs and secrets are signed with the organization's **Ed25519** keys, outside the nodes; signed documents go in COSE_Sign1 over CBOR (ADR 0025). The trust policy (which key signs which role) comes from a **bundle** signed by an offline root; the manifest's `[security.trust]` only restricts it. `jokerd` refuses unsigned artifacts in `mode = "prod"`.
- **Memory:** `oom_score_adj = -900` on `jokerd` and `+500` on the `joker-ml-*`/`joker-drv-*` processes, so that the kernel kills the model first and not the control.
- **Least privilege:** `jokerd` runs as user `joker`, with access through groups (`gpio`, `dialout`, `i2c`, `spi`, `video`) and `CAP_SYS_NICE` + `CAP_IPC_LOCK` (for RT and `mlockall`). It does not need root. Tier X drivers start as **transient systemd units** (`StartTransientUnit`), with `DynamicUser` by default and the systemd sandbox: this is how they run as another user without `jokerd` having `CAP_SETUID` (D20, `ref/06` §5.2).
- **Sandbox:** Wasm apps have no ambient authority (§9). Tier W drivers only see the transport they were given.
- **Network:** the local API listens on the Unix socket by default, and TCP is always mTLS (there is no switch to turn it off). Uplinks use TLS with mandatory certificate verification.
- **Commands from outside** (D16): writes through the API over TCP, through the tunnel of a remote session and through the agent protocol carry the sender's `nonce` and `ts`. A repeated nonce is always rejected; with the clock `synced`, so is a `ts` outside ±30 s. Without a synchronized clock, only the nonce and the binding to the target are checked, and the audit records it. The nonce memory never forgets: when full, it refuses requests (`ref/00` §9; document 09, attack 9). The **bridges** (OPC UA, Modbus, ROS 2) do not carry a nonce: they authenticate through their own protocol and write as declared writers, with level and lease.
- **Secrets** (ADR 0024): each secret is encrypted with HPKE to the X25519 key of each recipient node and signed per entry. `secrets.toml.enc` only has encrypted values and **may go into Git**; this is how the Deck distributes them without reading them. The node stores them encrypted and decrypts them into pinned memory at the moment of use. Apps request secrets by a local name bound in the manifest (`[apps.<app>.secrets]`), only in the async instance, and never read them from the manifest (`ref/02` §2.4).

---

## 18. Time (M14)

- **Scheduling:** `CLOCK_MONOTONIC`. Never the wall clock.
- **Timestamps:** UTC in ns, with **clock quality** per time generation: `Synced(ntp|ptp|gnss)`, `Holdover(since)`, `Unsynced`. Each sample stores `ts_acq` in relative monotonic time, and the translation to UTC uses the historian's correspondence table. A clock jump (NTP correcting 3 hours after a boot without an RTC) does not scramble the series: the correspondence is corrected retroactively, without rewriting samples. **Reserved in 1.0** (ADR 0081): the 1.0 historian stores the UTC instant computed at acquisition, and a clock correction does not change samples already recorded.
- **Apps:** `cx.now()` returns the tick time (monotonic and UTC, plus the quality). An app may declare `requires_clock = "synced"`, and in that case its outputs stay blocked while the clock is not synchronized: `Rejected(ClockUnsynced)` at step 0 of the envelope, and the output goes to `safe` (§7; 16, E1). This is the case of an hourly tariff.
- **PTP** (IEEE 1588 via `linuxptp`) for sites that need sub-ms correlation between devices.

---

## 19. Observability

Own metrics, exposed as `joker.*` signals (therefore recorded, alarmable and sendable like any other signal), via OTLP, in `GET /v1/metrics` and in the Prometheus text format at `GET /metrics` (ref/09 §5.2, ADR 0071):

| Area | Metrics |
|---|---|
| Cycle | start jitter (HDR histogram), tick duration, overruns, measured WCET per task |
| Drivers | latency per request, timeouts, reconnections, bytes, coalesced requests |
| Envelope | results per output (`accepted/clamped/rejected/suppressed/tripped`) |
| Arbiter | active level per output, leases about to expire, active manual overrides |
| Apps | traps, restarts, memory, async CPU time |
| ML | p50/p99 latency per model and backend, backend fallbacks, OOD |
| Store/link | queue, cursor lag, drops by policy, budget |
| System | temperature, CPU throttling, disk, memory, clock quality |

`jk top` shows all of this live, and `jk trace <signal>` follows a signal from the driver to the envelope.

**Decision log (normative rule).** Entries are logged per **change of result**, and not per tick: for each output, each change of the triple (applied value, result, active level) is kept, with the cause and the **repetition count** of the previous triple (how many consecutive ticks gave the same), and the instant of the first and of the last. An output stable at 100 Hz generates one entry when it changes, and not 100 per second. `fsync = "always"` (§12) only applies to the entries for **trips**, **manual writes**, **rejections** (`Rejected(*)`) and **changes of active level**; the others (`Accepted`, `Clamped`, `Suppressed`, `Overridden` without a change of level) go in the WAL batch. The trace (16 §7) still has every tick: the log is the auditable summary, the trace is the replay. For the outputs of an MCU, the log accepts **compacted** results: when the Joker Link window fills up, the pending results are merged by (output, level), the most recent one is kept, and the lost ones are counted (`ref/07` §8.2). Telemetry never stalls control, and what is lost is counted.

---

## 20. Failure modes

| Failure | Detection | Reaction | Where it shows |
|---|---|---|---|
| Equipment stops responding | driver timeout | signals `Bad(CommFailure)`; dependent permissives to false; outputs of the equipment: device fallback | alarm `joker.device.<d>.offline` |
| Absurd value (NaN, outside physical limits) | acquisition stage | `Bad(OutOfRange)` | quality |
| Frozen sensor with good quality | `stuck_detect` | `Uncertain(Frozen)` | quality, alarm |
| Write that never comes back | per-write timeout | latest-wins box; N timeouts → `Down` → device fallback | alarm |
| App in trap | host | proposals of the tick discarded; crashloop after K | alarm, `jk status` |
| Slow app in the cycle | epoch | interrupted; tick without that app | app overrun |
| Hung group | supervisor (group deadline × 3) | the supervisor sets the `force_safe` bit in the boxes of the group's outputs, and the drivers send `safe` without going through the group | critical alarm `joker.group.<g>.stalled` (ADR 0086) |
| Dead `jokerd` | systemd / hardware watchdog | restart; persistent trips restored; outputs start at `safe` | startup counter |
| Frozen board | hardware watchdog | reset; device fallback covers the gap | — |
| Slow model | model deadline | `Err(Deadline)` to the app; backend fallback after N | ML metrics |
| Hung GPU driver | watchdog of the `joker-ml-<id>` process | kill + restart; backend fallback | alarm |
| Disk full | store | aggressive retention; without blocking ingestion; if `< 1%`, only alarms and decisions | alarm `joker.store.disk_full` |
| Uplink network down | link | store-and-forward; budget recalculated | cursor lag |
| Clock jumps | time module | new correspondence; `Holdover`/`Unsynced` | clock quality |
| New config cuts the network | confirmation window | automatic rollback | generation log |
| Power outage | — | at startup: WAL, trips, manual levels (16, A14), last confirmed generation | — |

---

## 21. Target figures (to be validated)

| Metric | Target | Condition |
|---|---|---|
| `jokerd` binary, `minimal` build (`sim`, `sysfs`, Modbus, MQTT; `ref/06` §3.8) | ≤ 20 MB | aarch64, static, `strip`; no ML (1.0 has no Burn, ADR 0019) |
| `jokerd` binary, full build (`default`, `ref/06` §3.8) | ≤ 30 MB | same |
| Idle RSS without ML | ≤ 60 MB | 1,000 signals, 3 apps, historian cache of 8 MB (the resident `.text` counts) |
| Startup to the first tick | ≤ 2 s | AOT in cache |
| Tick jitter p99.9 | ≤ 100 µs | CM4/CM5, PREEMPT_RT, isolated CPU, 1 ms group |
| Tick jitter p99.9 | ≤ 2 ms | kernel without RT, 100 ms group |
| Overhead of the cycle Wasm call | ≤ 5 µs | empty task, Cranelift |
| Envelope + arbiter per output | ≤ 300 ns | no atomic groups |
| Ingestion of scalar samples | ≥ 100 k/s | CM4, 1 `io` CPU core |
| Historian ingestion | ≥ 200 k samples/s | CM4, eMMC |
| App hot swap | 0 ticks lost | same port signature |

**Binary size in CI.** The two builds (`minimal` and full) are measured on every commit with `cargo bloat --release --crates` for `aarch64-unknown-linux-gnu`; a build above the table's limit makes CI fail, with the crates that grew the most. Raising a limit is an explicit decision, in the same commit that justifies it. The builds and their limits are `xtask/size-limits.toml`, and the gate is `cargo xtask size`, which measures the stripped binary. Until `jokerd` has the Cargo features of ref/06 §3.8 (AUR-432), only the full build exists, and the file lists only it; the `minimal` build joins the gate with its features.

---

## 22. Platforms

| Tier | Platform | Planned status |
|---|---|---|
| **1** | Linux aarch64: Raspberry Pi 4/5, CM4/CM5, RevPi Connect 4/5, Jetson Orin | CI with hardware, RT measured |
| **1** | Linux x86_64: industrial PCs | CI with hardware |
| **2** | Linux aarch64 without hardware in CI: Raspberry Pi 3 B+ (the bench, document 23) | Cranelift; RT measured on the bench, no target |
| **2** | Linux armv7: RevPi Core 3, older gateways | Pulley instead of Cranelift; no ML on GPU |
| **2** | Linux riscv64 | Cranelift; no hardware in CI |
| **dev** | macOS, Windows (WSL2) | `sim` drivers, Burn Metal/Vulkan; never in production |
| **dev** | Linux armv6: Raspberry Pi Zero W (bench) | Pulley, if wasmtime compiles for ARMv6 (to be confirmed in Phase 1); otherwise only native apps, and `jk check` warns with `J0243` (ref/01 §10.2); 1 CPU core, no RT |
| **dev (MCU)** | original ESP32 (Xtensa LX6, bench) | Joker Micro for development; Espressif's Xtensa toolchain |
| **1 (MCU, M1)** | RP2350, STM32G4 | **Joker Micro**: the same `no_std` core, Embassy, native Rust apps, Joker Link (document 19) |
| **2 (MCU, M2)** | ESP32-C6/S3, STM32H5/H7 | Joker Micro with network (Wi-Fi, Ethernet): standalone nodes |
