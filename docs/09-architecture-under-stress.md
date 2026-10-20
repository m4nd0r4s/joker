# 09 — Architecture under stress

Document 08 attacked the design with use cases. This one attacks the **architecture itself**: whether the abstraction holds up against real protocols, whether the time and memory budgets close, what happens when each piece fails, whether real time on Linux is credible, whether the system extends without touching the core, how it survives versions and attackers, and whether the "wildcard" does not become a Swiss army knife impossible to maintain.

Each attack ends with one of three conclusions: **holds**, **holds with refinement** (change already incorporated in documents 01–07) or **new primitive** (M-n, document 10).

---

## Attack 1 — Where the signal abstraction lets detail slip through

Each protocol in the catalog was taken and the question asked: what can the protocol express that the signal cannot?

| Protocol semantics | Example | Fits? | How |
|---|---|---|---|
| value with timestamp and quality | OPC UA DataValue | yes | it is the origin of the model |
| push on change | OPC UA subscription, BACnet COV, CAN PDO, GPIO edge | yes | `subscribe` in the driver |
| **commands** with arguments and result | OPC UA method, Modbus reset by pulse, CANopen SDO, calibration | **no** | **M19 actions** |
| **coherent and ordered multi-register writes** | u32 in 2 registers, mode+setpoint, "write parameters and then commit" | **no** | **M20 atomic groups and order** |
| read with side effects | registers that clear alarms when read | yes, with care | `forbidden` in the profile; read only as an action |
| structured types | OPC UA ExtensionObject, S7 structures | partial | flattened into prefixed paths; fields read together share `acq_id` |
| large arrays | spectrum from a power analyzer, temperature profile | partial | as a block signal (M7) with `period` = spatial step, or as an event |
| object dictionary with thousands of parameters | CANopen OD, drive parameters | yes, with refinement | `param` direction with `poll = "on_demand"`: read at `connect` and when requested, not in a cycle |
| equipment alarms and events | OPC UA A&C, BACnet events | yes | **event** form (document 02, §1) |
| history in the equipment | OPC UA Historical Access, inverter logs | no | **boundary**: Joker does not import third-party history in the core; an action can fetch it |
| file transfer, firmware | inverter FTP, block SDO | yes | long action with progress (M19) |
| stateful sessions with per-user authentication | OPC UA with a user per operator | partial | Joker connects with a service identity; the operator's identity stays in the decision log, and does not reach the equipment |
| priorities in the equipment itself | BACnet priority array | yes | M15 + `safe = "relinquish"` |

**Conclusion: new primitives M19 and M20.** The rest fits, some cases with refinement (`param` with on-demand poll, history as an action). The abstraction loses detail at two points (per-user sessions and history in the equipment), and both are declared in document 12.

---

## Attack 2 — The hot-path budget

A heavy reference site: **5,000 signals**, 40 devices, 3 cycle groups (1 ms with 1 app, 100 ms with 4 apps, 1 s with 3 apps), 300 outputs, historian at 1 Hz on all signals and at 10 Hz on 500.

### 2.1 First version, and why it did not close

The first version of the WIT contract had only `read(port)` and `write(port, v)` as host calls. An app in the 100 ms group with 200 inputs and 50 outputs makes **250 host calls per tick**. A host call in wasmtime with the canonical ABI costs tens to hundreds of ns, depending on the type (to measure). With `variant value` and `record sample` crossing the boundary, the estimate came to ~150 ns per call, that is ~40 µs per tick on the boundary alone. It is not fatal at 100 ms, but it is the dominant term, and at 1 ms with 100 inputs it is ~15 µs (1.5% of the period), more than all the rest of the infrastructure combined.

### 2.2 Refinement: packed image

`on-cycle` now receives the app's **packed input image** and returns the **packed proposals**:

```wit
on-cycle: func(task: u32, image: list<u8>) -> cycle-result;
```

- The layout is fixed at `init` (the port order comes from `joker:meta`): a 64 B header, **24 B per sample** (value 8, quality code and reason 1 + 1, flags 2, `seq` 4 truncated, `ts_acq` 8) and 24 B of result per output, which give `last_outcome()` without host calls.
- The host writes the image into a pre-allocated arena and the canonical ABI copies it in a single operation into a guest buffer reserved at `init`. **In `on-cycle` the app does not allocate:** an allocation is trap `J-E0903` (the first version used a *bump* allocator reset every tick, which left dangling pointers in the state; ref/04 §4.13).
- Proposals come back in a buffer of **typed records**: `WRITE` of 14 B (`kind u8 · flags u8 · port u32 · value u64`), `METHOD` of 10 B + arguments, `PUBLISH`, `COUNTER_ADD`, `EMIT` and `NOTE`. The port is `u32` (an app is not limited to 65,535 ports) and class methods travel in the same buffer (document 18, P1).
- Tier 2 SDKs use the same image. `cycle-io.read/write` remain for dynamic access and for values that do not fit in `u64` (strings, bytes, records).

The exact layout, the validation and the errors are in ref/04, §3.

### 2.3 Resulting budget (estimates to validate on the CM4)

| Stage | 1 ms group (1 app, 60 in, 12 out) | 100 ms group (4 apps, 800 in, 150 out) |
|---|---|---|
| latch (10 ns/signal) | 0.6 µs | 8 µs |
| expressions (permissives, `calc`) | 1 µs | 15 µs |
| pack and copy the image | 0.5 µs | 6 µs |
| Wasm call (entry/exit) | 1.5 µs | 4 × 1.5 = 6 µs |
| app logic | 20 µs (declared) | 4 × 200 µs = 800 µs |
| arbiter + envelope (300 ns/output) | 3.6 µs | 45 µs |
| commit + queues + trace | 1 µs | 10 µs |
| **total** | **~28 µs (2.8% of the period)** | **~890 µs (0.9%)** |

The infrastructure stays below 10 µs in the 1 ms group, and the dominant cost becomes the app logic, which is what it should be. **Holds with refinement** (incorporated in documents 01, §9, and 04, §2; closed form in ref/04, §3).

### 2.4 Throughput

| Flow | Rate | Estimated cost | CPU core |
|---|---|---|---|
| ingestion (40 devices, 50 ms average) | ~100 k samples/s | ~200 ns each → ~2% of an `io` core | `io` |
| historian (5,000 × 1 Hz + 500 × 10 Hz) | 10 k/s | negligible | `bg` |
| uplink (Sparkplug, 1 min, with alias) | ~83/s | negligible | `bg` |
| `calc` expressions at 100 ms | 200 × 10/s | negligible | `rt` |

The real bottleneck of a site like this is the **buses** (Modbus RTU), not the CPU. That is why the bus budget in `jk check` (document 05, §5) is worth more than any optimization of the core.

---

## Attack 3 — Memory and footprint

### 3.1 Budget on a CM4 (2 GB) for the reference site

| Component | Estimate |
|---|---|
| `jokerd` `.text` (shared pages, read-only) | 20–30 MB |
| slots (5 000 × 32 B) + images + arenas | < 1 MB |
| SPSC queues and trace rings | 4 MB |
| Wasm instances (8 apps × 2 instances × 4–16 MiB) | 64–256 MB **virtual**, ~20 MB resident |
| historian: block caches + WAL | 32 MB |
| uplink and API | 8 MB |
| small TCN model (Burn CPU) | 5–20 MB |
| **total RSS** | **~100–130 MB** |

### 3.2 The problem that only shows up on 32 bits

By default, wasmtime reserves **4 GiB of virtual space per linear memory**, with guard pages, to eliminate bounds checks. In a 64-bit process that costs nothing. **On a 32-bit armv7 it is impossible**: the whole address space of the process is ~3 GiB. The engine configuration now depends on the architecture: on 32 bits, memories with explicit bounds checking, without guard pages and with `memory_reservation` equal to the declared `memory.max`. It costs ~10–20% more performance, which adds to Pulley. **Holds with refinement** (document 01, §9).

### 3.3 Streams

The pool of a 1080p NV12 camera with 6 buffers takes ~19 MB, and a 4K camera takes ~75 MB. `jk check` adds up the pools and compares them with the board's RAM (`J0380`, document 08, round 5).

### 3.4 OOM

- `jokerd` runs with `oom_score_adj = -900`, and the `joker-ml-*` and `joker-drv-*` processes with `+500`. In an out-of-memory situation, the kernel kills the model first and not control.
- Wasm instances have hard limits (pooling), and an app that tries to grow beyond `memory.max` gets a `memory.grow` failure (and usually traps), without affecting anyone else.

---

## Attack 4 — Fault injection matrix

For each component and each failure mode: who detects it, what happens, and whether the original design had an answer.

| Component | crash | hang | slow | wrong value | exhaustion |
|---|---|---|---|---|---|
| driver (read) | driver restart; `Bad(NotConnected)` | timeout per batch | `Degraded` | **frozen sensor with `Good`** → see (b) | full ingestion queue → counted drop |
| driver (write) | same | **write that never returns** → see (a) | accumulated writes → see (a) | readback (`verify`) | — |
| app (cycle) | trap → tick without the app | epoch | app overrun | envelope | `memory.max` |
| app (async) | trap → restart | no effect on the cycle; lease expires | same | envelope | `memory.max` |
| model | isolated process restarts | deadline | backend fallback | OOD guard (partial) | OOM → process killed first |
| cycle group | the group is a `jokerd` thread | **supervisor** → see (c) | overrun | — | — |
| historian | service restarts | does not block the core (queues) | counted drop | — | full disk → retention |
| uplink | same | same | backlog in the historian | — | budget |
| `jokerd` | systemd + device fallback | hardware watchdog | — | — | oom_score_adj |
| board | hardware watchdog | same | thermal throttling → M8, alarm | — | — |

Three answers that were missing:

**(a) Write that hangs.** A piece of equipment accepts the TCP connection and never answers the FC16. With a FIFO queue of writes to the driver, the following writes pile up and the first one to go out, when the equipment wakes up, is the oldest. **Refinement:** the output queue for each driver is a **per-output box with "the last one wins"** (latest-wins), not a FIFO. Only scheduled writes, pulses and atomic groups use an ordered queue, and that one is bounded. Each write has a timeout, and after N timeouts the device goes to `Down`, with the device fallback covering.

**(b) Frozen sensor with `Good` quality.** It is a failed 4–20 mA transmitter, or an intermediate PLC that stopped updating but keeps responding. The value does not change for hours and the quality says `Good`. **Refinement:** `stuck_detect = "30m"` per signal (and, in profiles, for quantities that always vary, such as grid frequency): a value bit-for-bit identical for that time becomes `Uncertain(Frozen)`. New quality reason in document 02, §4.

**(c) Supervisor and hung group.** The supervisor puts the outputs of a hung group into `safe`, but whoever writes into the driver queues is the group, and the single-writer principle prevents the supervisor from writing there. **Refinement:** each output box has an atomic **`force_safe`** bit, which only the supervisor can set and which the driver checks before each send. The emergency path does not go through the group and does not violate the single writer of the values.

**Holds with refinement** (documents 01, §3 and §20, and 02, §4).

---

## Attack 5 — Real time on Linux: what can go wrong

A p99.9 of 100 µs in a 1 ms group is only achievable if none of the following things happen on the `rt` core:

| Latency source | Mitigation in the design | Verification |
|---|---|---|
| page faults | `mlockall(MCL_CURRENT\|MCL_FUTURE)`, pre-touched stacks, pre-touched cycle instances | `jk doctor --rt` counts *major faults* for 60 s |
| allocation in the host during the tick | invariant 5 of `AGENTS.md` | *no-alloc* guard in debug and in CI (`assert_no_alloc` on the tick path) |
| locks and priority inversion | no lock on the tick path; seqlocks and SPSC | review + stress test with load on `io`/`bg` |
| frequency scaling | `performance` governor on the `rt` core | `jk doctor` |
| interrupts | IRQ affinity off the `rt` core (except the EtherCAT NIC, which goes *to* the `rt/drv` core) | `jk doctor` reads `/proc/interrupts` |
| SMIs (x86) | cannot be mitigated in software | `hwlatdetect` in `jk doctor --rt`; boards with SMIs above 50 µs are marked `rt: unsuitable` |
| thermal throttling | M8: training and ML off `rt`, temperature alarm | `joker.sys.temp` signal |
| wasmtime: lazy initialization | first call made at `init`, with the instance warmed up | measurement of the first tick |
| wasmtime: traps use signals | signal handler installed before raising the priority | trap test under load |
| logs from the cycle | lock-free ring, drained by the `svc` thread | — |
| disk writes from the cycle | forbidden; the trace goes to a ring, drained in `bg` | — |

**Honest conclusion:** on a kernel without PREEMPT_RT, the 1 ms targets are not guaranteed, and `jk check` rejects groups below 10 ms without `kernel = "preempt_rt"` (`J0351`). With PREEMPT_RT, which has been in the mainline kernel since 6.12, and an isolated CPU, the target is plausible on CM4/CM5 and x86, but it **must be measured per board** before being promised. That is why the board profile carries the result of the reference `jk doctor --rt`. **Holds, on condition of measuring.**

---

## Attack 6 — The Wasm instance model

A wasmtime `Store` is not `Sync`. An app with cycle tasks (RT thread) and async tasks (tokio) cannot use the same instance in parallel. The alternatives evaluated:

| Option | Problem | Decision |
|---|---|---|
| one instance with a mutex | an async task holding the `Store` during an inference blocks the tick → **priority inversion** on the RT path | rejected |
| one instance, with the async tasks running *between* ticks on the RT thread | puts async work on the RT core and blows the budget | rejected |
| Wasm threads with shared memory | they are not stabilized in the Component Model and would bring locks back inside the guest | rejected for now; revisit when *shared-everything threads* stabilize |
| **two instances of the same component**, which share only internal signals and mailboxes | state is not shared implicitly, and that has a DX cost | **accepted** |

The DX cost is real, and the SDK must make it obvious:

- `CycleIo<T>` and `TaskIo<T>` are different types, and the state of a cycle task (`st: &mut Ctl`) is not visible to the async tasks;
- `Internal<T>` and `Mailbox<T, N>` are the only channels, and the SDK documentation opens with a diagram that shows them;
- the compile error for a shared `static mut` explains why.

The unexpected advantage: **isolation gives robustness**. A trap in an async task does not kill the cycle instance, and a memory leak in an async task does not touch the cycle. **Holds**, with the DX cost accepted (ADR 0004).

---

## Attack 7 — Extensibility: adding without touching the core

| I want to add… | Extension point | Touches `joker-core`? | ABI |
|---|---|---|---|
| a new protocol | `Driver` trait (N), `joker:driver` world (W), JDP (X) | no | N: internal (build feature); W: versioned WIT; X: versioned JDP |
| a new piece of equipment | TOML profile | no | versioned profile schema |
| a new board | board profile | no | same |
| a new accelerator | `InferenceBackend` trait | no | internal, or plugin with C ABI (`joker-ml-abi`) |
| a new language for apps | any toolchain that produces a component for `joker:runtime` | no | WIT |
| a pipeline operation | `PipelineOp` trait with format negotiation | no | internal |
| an uplink type | `Uplink` trait | no | internal |
| a bridge | `Bridge` trait (must declare a writer, M11) | no | internal |
| a function in expressions | **yes** | yes | the expression language is closed on purpose |
| an envelope step | **yes** | yes | requires an ADR |

On the native ABI: Rust has no stable ABI, and dynamically loaded native Rust plugins are a known source of subtle failures. That is why **tier N drivers and backends are compiled in** (a custom build of `jokerd` with the chosen features, `jk runtime build --features ethercat,bacnet`), and third-party extension is done through **Wasm** (W) or through a **process** (X, JDP). The only dynamic native plugin is the generated models (document 06, §3.2), with a minimal, verified C ABI. **Holds.**

---

## Attack 8 — Versions and compatibility (M21)

There are seven versioned things that cross paths on a device: runtime, the apps' WIT world, manifest schema, lock format, profiles, `.jkm` format (and the Burn version inside it) and the W drivers' WIT world.

The scenario that broke the first version: a fleet with half of the devices on runtime 0.3 and half on 0.4, and a new app compiled against `joker:runtime@0.4.0` that uses a new function. What happens when the fleet agent sends it to a 0.3?

**Policy (M21):**

| Artifact | Rule |
|---|---|
| `joker:runtime` WIT world | semver. Until 1.0, the runtime supports the current world and **the previous minor**, through a generated adapter (component composition). **After 1.0, it serves every `joker:runtime@1.y` with y ≤ its own** (golden rule, ADR 0022). A world *newer* than the runtime is rejected **before deploy** by `jk` and by the agent (`J-E0001`, from the metadata). |
| manifest schema | versioned with the runtime: `joker = "0.4"` in `[project]` is the minimum version of the runtime and, therefore, of the schema. Until 1.0, `jk migrate` rewrites manifests between minors, and the runtime accepts the current minor and the previous one; in the 1 series, a 1.x runtime accepts any 1.y manifest with y ≤ x (ref/01 §9). |
| `joker.lock` | `version = 1` throughout the 1.x series; a minor only adds sections and optional fields, and `jk` writes only the subset of the project's `joker` version. v2 reads `version = 1` and writes `version = 2` after `jk migrate` (ref/02 §3.1.6). |
| profiles | `name@major`; the lock pins the exact version. |
| `.jkm` | `format = N` in `model.toml`, and `burn = "0.22.x"` in the Burn payload. The runtime rejects a Burn from another minor (`J0502`) and suggests `jk model migrate`. |
| `joker:driver` world | like `joker:runtime`, with the same version as the `joker:runtime` it imports (ref/06 §4.8). |
| JDP | version negotiation in `hello`; `jokerd` supports the current version and the previous one (ref/06 §5.4). |

**Consequence for the roadmap:** until 1.0, each runtime minor may break the WIT world, but never without an adapter for the previous minor. After 1.0, the major world is stable for the whole major, and the adapter window covers every 1.y. The interfaces reserved for 1.1 (`ml`, `frames`, scheduled writes, intents) stay `@unstable` outside the stable world, so that their shape is not frozen before they exist (ref/04 §2.8–§2.9). **New primitive: M21.**

---

## Attack 9 — Security: threat model

**Assets:** the physical process (the most important), the device credentials, the process data and the models (intellectual property).

| Adversary / failure | Attack | Mitigation | Residual |
|---|---|---|---|
| buggy or malicious app | escape the sandbox | Wasm without ambient authority; only bound handles; no FS/network by default | bugs in wasmtime (small, audited surface) |
| same | DoS on the cycle | epoch, `memory.max`, admission by WCET | — |
| same | **doing harm inside the envelope** (e.g. cycling the battery or a contactor to wear it out) | rate, `min_interval` and **actuation budget** (`max_actuations = "20/h"`) | an authorized app can always make bad choices within the limits: it is the cost of authorizing it |
| compromised cloud / uplink | send commands | remote writes are declared writers with level and lease; mTLS; nonce + timestamp with a ±30 s window against replay | valid commands within the envelope pass |
| attacker on the local network | **talk directly to the equipment** (Modbus TCP has no authentication) | none in Joker | **boundary**: network segmentation (field VLAN, Joker as the only host with two NICs); document 12, §5 |
| same | attack the local API | Unix socket by default; TCP only with mTLS; roles | — |
| physical access | read the key, swap the SD card | TPM/secure element when there is one; signed artifacts | secure boot belongs to the OS (document 12) |
| supply chain | tampered artifact | signatures per artifact type; SBOM; reproducible builds as a goal | compromised signing keys |
| poisoned model | on-device training with manipulated data | lineage (M10), mandatory validation, shadow (M16) | subtle poisoning attacks pass validation |

Two refinements came out of this: the **actuation budget** in the envelope (step 6, document 01, §7) and the **single-client rule** of attack 11. **Holds with refinement.**

---

## Attack 10 — Determinism and replay: where it falls short

| Source of nondeterminism | Treatment |
|---|---|
| clock | input (ADR 0007) ✓ |
| cycle inputs | snapshot in the trace ✓ |
| parameters and config | generation in the trace ✓ |
| model outputs on GPU/NPU | recorded (M22) ✓ |
| **floating-point arithmetic in Wasm** | Wasm is deterministic, except in the NaN bits and in **relaxed SIMD**. **Refinement:** relaxed SIMD is disabled in cycle instances and NaNs are canonicalized (`cranelift_nan_canonicalization`). It costs little and guarantees that the same `.wasm` gives the same bits on aarch64 and x86_64 |
| randomness in apps | `random` with a seed coming from the host and recorded ✓ |
| **async tasks** | they read live slots, with an order and timing that depend on the scheduler. Replay by default injects the recorded async **proposals** at the tick in which they arrived (effects at the boundary). With `trace.async = true`, the reads of the async tasks are also recorded and replay becomes exact, at a cost that stays visible |
| native apps | reproducible only on the same architecture |

**New primitive: M22** (nondeterministic outputs recorded; replay policy for async). With refinement in the Wasm engine flags.

---

## Attack 11 — Scale: where the design meets limits

| Dimension | Design limit | What breaks first |
|---|---|---|
| signals per Joker device | ~100 k (3.2 MB of slots) | ingestion and the buses, long before |
| cycle groups | ≤ 3 per `rt` core (rate-monotonic) | admission (M23) |
| apps | ~50 (pooling, 2 instances each) | virtual memory on 32 bits |
| API subscriptions | ~1,000 WebSocket clients | the `bg` CPU |
| **connections to a Modbus TCP device** | the equipment accepts 1–4 connections | **SCADA and Joker competing for connections**, with random timeouts on both sides |

The last row is a classic field problem. **Refinement — single-client rule:** on a site with Joker, Joker should be the **only client** of each field device, and the other systems read through the bridges (Modbus server and OPC UA server, M11). `jk doctor` detects concurrent connections when the equipment allows it (session counters in some profiles), and document 12 recommends the rule explicitly. **Holds with refinement.**

---

## Attack 12 — Wildcard or Swiss army knife? The cost of complexity

Document 10 lists 23 primitives (24, with the equipment classes of document 14, which are optional and compile to ports). The risk is that Joker becomes what it criticizes: an enormous system, hard to learn and to maintain.

### 12.1 The cost of each primitive

| Primitive | Crate | Optional in the build? | Touches the hot path? |
|---|---|---|---|
| M1 device fallback, M2 lock | manifest, hal | no | no |
| M3 causal tracing | core | no | yes: bitset of reads per tick (~100 ns) |
| M4 streams, M5 pipelines | ml, drivers | **yes** (`vision`) | no (ml domain) |
| M6 scheduled writes | core, hal | no | yes, only on outputs with `schedule_depth > 0` |
| M7 blocks | core, store | **yes** (`waveforms`) | no |
| M8 resource classes | jokerd | no | no |
| M9 alarms | core | no | yes: evaluation per tick (bytecode) |
| M10 lineage, M16 shadow, M17 guard | ml | **yes** (`ml`) | no |
| M11 bridges | link | **yes** per bridge | no |
| M12 budgets, M13 overlays, M14 clock | link, manifest, core | no | M14: flag per sample |
| M15 arbiter | core | no | yes (~50 ns per output) |
| M18 proposals | api | no | no |
| M19 actions, M20 atomic | core, hal | no | M20: yes, only on grouped outputs |
| M21 versions, M22 determinism, M23 admission | host, manifest | no | no |
| M24 classes and instances | manifest, sdk, core (queries) | no | only the queries, as derived signals |

**`joker-core` must fit in ~15 k lines** (target, to watch in review). Everything that is domain (vision, waveforms, ML, bridges) lives in optional crates, and a `minimal` build (the list is in ref/06 §3.8, and the size target in 01 §21) excludes them.

### 12.2 Progressive disclosure in the DX

The minimal manifest has 16 lines, and a beginner only needs 4 concepts (signal, port, binding, envelope). `mode = "dev"` is what lets it run without the equipment fallback verified; in `prod`, the same GPIO output without a fallback is `J0205` (ref/01):

```toml
[project]
name = "bomba"
mode = "dev"

[devices.io]
driver = "gpio"

[devices.io.signals]
line_4 = { line = 4, alias = "tank.low" }
line_5 = { line = 5, alias = "pump.run", direction = "output" }

[outputs."pump.run"]
safe = false

[apps.level]
path = "apps/level"
bind = { low = "tank.low", pump = "pump.run" }
```

Arbiter, shadow, pipelines, overlays and budgets show up **when they are needed**, with safe defaults and a `jk check` that explains each one. It is the same discipline as Cargo: the minimal `Cargo.toml` has 3 lines, and the maximal one has hundreds.

### 12.3 Conformance as containment

The envelope, the arbiter, the process image and the quality semantics have a **data-driven conformance suite** (`conformance/<area>/<suite>.conformance.toml`, with the single schema of ref/03 §2.6: cases with the expected result written by hand from the spec) and property-based tests. It is what allows adding primitives without breaking the guarantees. **Holds, with permanent vigilance.**

---

## Summary

| # | Attack | Conclusion | Result |
|---|---|---|---|
| 1 | abstraction leaks | new primitive | **M19** actions, **M20** atomic groups and order; `param` with on-demand poll |
| 2 | hot-path budget | refinement | packed image in `on-cycle`; typed proposal records (ref/04 §3) |
| 3 | memory | refinement | wasmtime configuration for 32 bits; `oom_score_adj` |
| 4 | fault injection | refinement | latest-wins boxes for drivers; `stuck_detect`/`Frozen`; supervisor's `force_safe` bit |
| 5 | real time | holds, with measurement | `J0351`; `jk doctor --rt`; no-alloc guard in CI |
| 6 | Wasm instances | holds | two instances per app (ADR 0004) |
| 7 | extensibility | holds | tier N compiled in; third parties via W/X |
| 8 | versions | new primitive | **M21** |
| 9 | security | refinement | actuation budget; fieldbus boundary |
| 10 | determinism | new primitive | **M22**; relaxed SIMD off and NaNs canonicalized |
| 11 | scale | refinement | single-client rule via bridges |
| 12 | complexity | holds, with vigilance | builds by features; progressive disclosure; conformance |

**M23** (real-time admission by WCET) comes from attacks 2 and 5 together and is described in document 01, §4.
