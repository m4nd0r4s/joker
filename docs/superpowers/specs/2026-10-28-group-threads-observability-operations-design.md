# Group threads, runtime signals and node operation: design

Date: 2026-10-28. Issues: AUR-396 and AUR-274 (part A), AUR-271 (part B), AUR-259 (part C).
The owner delegated the decisions in this document ("faça o desenho de tudo"); every
choice below is the designer's call, checked against the normative documents.

The three parts are independent projects, built and merged in the order A, B, C.

---

## Part A — one thread per cycle group

### Why

docs/01 §2 already says `rt/<group>`, one thread per cycle group, `SCHED_FIFO` with a
rate-monotonic priority, pinned to an `rt` CPU. jokerd runs every group on a single
`rt/cycle` thread. Two consequences the owner sees on the bench:

- a hang in one group stops every group, and the supervisor forces every output of the
  node to `safe`, not just the hung group's (AUR-274's open done-when);
- a slow 1 s group delays a 10 ms group: their ticks share one thread (AUR-396).

### Goals

- Each cycle group ticks on its own thread, `rt/<group>`, when the pacer is the real clock.
- A group that hangs or panics leaves the other groups ticking. The supervisor forces only
  the hung group's outputs to `safe`, and raises only that group's `stalled` alarm.
- `SCHED_FIFO`, rate-monotonic priorities below the supervisor's, every group thread
  pinned to `[resources] rt.cpus`.
- Replay and tests stay deterministic (invariant 9): the same trace gives the same bytes.

### Non-goals

- Restarting a dead group thread in place. The spec does not ask for it: docs/01 §20
  answers a hung group with `force_safe` and the critical alarm, and ADR 0061 then stops
  the watchdog feed so the board restarts. A thread cannot be killed safely in Rust, and
  a half-restarted group could act on the plant with lost state. Recovery is a new
  generation, a rollback or a restart. (AUR-396's "restarts it" task is closed by this
  ruling, with the reason written on the issue.)
- Wasm epoch deadlines (nobody increments the engine epoch in jokerd today). That is
  separate work in the host; it is noted, not built here.

### Anti-goals

- No lock on the tick path. Cross-thread traffic stays on the existing lock-free queues
  and seqlock slots.
- No change to `joker-core`'s semantics. The core types already work per group
  (`Outputs::commit`, `Classes::evaluate_derived` and `Alarms::evaluate` each skip what
  another group owns).

### Design

1. **The pacer chooses the mode.** `Pacer` gains `fn fork(&self) -> Option<Box<dyn Pacer>>`.
   `SystemPacer` returns a copy on the same monotonic origin and the same clock-quality
   cell. `VirtualPacer` returns `None`. With a fork for every group and more than one
   group, `cycle::run` starts one scoped thread per group. Otherwise it keeps today's
   single-thread interleaving: the earliest deadline first, ties by group index. Replay
   and the tests on a virtual clock stay byte-for-byte deterministic.

2. **Per-group state, built once before the first tick.** The coordinator (`rt/cycle`)
   builds the generation's tables, as today, and then one `GroupRun` per group. Each
   `GroupRun` owns:
   - an `Outputs` over that group's output configurations only, plus the atomic groups
     that group owns. J0272 already guarantees one owner group per atomic group.
   - its own `Classes` and `Alarms`, each with its own state arena. The core already
     evaluates only the image's group; the instances of other groups stay unused.
   - its own `Verifier`.
   - one `CommitTx` per device, bound only to that group's outputs (see point 3).
   - its process image, tasks and trace scratch, as today.

   In single-thread mode, the same `GroupRun`s are ticked in turn, so both modes run the
   same code.

3. **Commit channels per group.** `joker-hal` gains `CommitTx::owned(outputs, bindings,
   boxes, last)`. It binds only the bindings whose output is in `outputs`, and skips the
   rest, which belong to another group. Each box keeps a single writer, the owning group.
   `CommitRx` reads boxes by their own sequence numbers, so independent counters per
   group are safe. Change made by the HAL front, in its own commit.

4. **Cross-group alarm trips.** An alarm's `actions.trip` may name an output of another
   group. `Outputs::trip` on a foreign output returns `false`, so jokerd delivers the trip
   to the owner group in two ways:
   - **Edge.** When the alarm enters `active`, the `Sink` pushes a `TripRequest` into the
     owner group's existing `shared.trips[g]` queue. A full queue is counted
     (`trip_requests_dropped`).
   - **Level.** On each of its ticks, the owner group reads the alarm's
     `alarm.<a>.active` slot and trips the output while it reads `true`. This keeps the
     documented rule that "a reset of an output tripped by an alarm that is still active
     trips again on the next tick".

   The edge covers an activation shorter than the owner's period; the level covers a
   dropped request and the re-trip after a reset. The latency is at most one period of
   the owner group. That is what `Outputs::trip` already documents ("takes effect at the
   next commit of the owner group").

5. **Queues that had one consumer become per group.** Today `rt/cycle` drains them; with
   one thread per group, each consumer must be the group that owns the target:
   - `alarm_commands` becomes one queue per group. The API routes an ack or shelve to the
     alarm's group.
   - `confirmations` becomes one queue per group. `Shared::push_confirmation` routes by
     the output's owner group. The output slot keeps a single writer.

   All the other queues the cycle produces into (`results`, `decisions`, `alarm_events`,
   `trace_filled`, `exports`, `reset_results`, `method_results`) are crossbeam
   `ArrayQueue`s, multi-producer already.

6. **Scheduling.**
   - Every group thread is named `rt/<group>` and pinned to `rt.cpus`.
   - It runs `SCHED_FIFO` with a rate-monotonic priority, through `thread-priority` as the
     supervisor already does: distinct periods sorted ascending get priorities 90, 89, …
     (never below 2). Equal periods share a priority.
   - The supervisor stays at the maximum.
   - A failure (no `CAP_SYS_NICE`) prints one warning per thread and keeps the default
     policy, the existing convention.

7. **Stop, `max_ticks`, exit.**
   - Every thread watches `shared.stop`.
   - `max_ticks`, still "over all groups", becomes an `AtomicU64` that each thread claims
     a tick from before running it; a thread whose claim fails ends.
   - `shared.running` is set by the coordinator around the whole scope.
   - The coordinator joins every group thread on its own. A panicked group is named in
     the run's exit (`CycleExit::panicked`) and logged; groups that ended cleanly give
     their applied values, and the panicked group gives none, so the next generation
     starts its outputs at `safe` (A8). The switch is not refused: that would leave the
     node with no generation running.

8. **Supervisor.**
   - Detection does not change (`stale_groups`).
   - The response narrows to the stale groups: `force_safe(shared, &group_boxes[g])` for
     each stale `g`, and the `supervisor_force_safe` record names them, as it already
     does.
   - The `stalled` alarm (ADR 0086) narrows with it, as that ADR anticipated.
   - ADR 0061's watchdog rule is unchanged: once any group was forced, the feed stops and
     a hardware watchdog restarts the board. Until then, or without a hardware watchdog,
     the other groups keep running.

### Spec changes (ADR 0089)

One ADR, "one thread per cycle group":
- mode by clock (point 1);
- cross-group trip delivery (point 4);
- per-group routing (point 5);
- priorities (point 6);
- panic handling (point 7);
- supervisor narrowing (point 8).

It amends ADR 0061 decision 27 ("every output" becomes "the outputs of the stale
groups"). docs/01 §2 needs no text change, because it already describes this model; §3.4
gains one sentence on cross-group trips.

### Tests (done-when of AUR-396 and AUR-274)

- **Integration, real clock.** Two groups (`fast` 10 ms, `slow` 50 ms). The task of
  `slow` blocks 400 ms. Within 3 × 50 ms plus a poll, the outputs of `slow` are
  force-safe and `joker.group.slow.stalled` is active. `fast` keeps ticking: its tick
  count grows during the hang, and none of its boxes is forced.
- **Integration, panic.** A task panics. Its group stops, its outputs are forced by the
  supervisor, and the other group keeps ticking. `Running::stop` returns the panic
  error.
- **Unit tests:**
  - `CommitTx::owned` binds only owned outputs;
  - a cross-group alarm trip takes effect on the owner's next tick (edge), and re-trips
    after a reset while still active (level);
  - an ack routed to a group's queue reaches the right group.
- **Determinism.** The existing replay tests pass unchanged on the single-thread path.
- **Allocation guard.** The existing tick guard runs inside each group thread.
- **Bench, Pi 3.** Two groups, one with a hanging native task, behind a test-only flag.
  This uses the `Hanging` provider pattern of `tests/api.rs` over the real daemon.
  `jk status` shows `slow` stalled, the fast LED blink group keeps blinking, and
  `ps -L` shows `rt/fast` and `rt/slow` with their `SCHED_FIFO` priorities.

---

## Part B — the runtime's own signals (`joker.*`) and the rest of AUR-271

### Why

docs/01 §19 says the metrics "are `joker.*` signals (therefore recorded, alarmable and
sendable like any other signal)", and ref/09 §5.2 says they are read with
`/v1/values?sel=joker.**`. On Linux, no document lists those paths, their types or who
writes them. ref/07 §8 lists only the MCU's minimum set. The registry holds no `joker.*`
signal, so `jk read 'joker.**'` returns nothing.

### Decisions (ADR 0090, "the runtime's signals on Linux")

1. **Where they live.**
   - The plan adds the runtime signals to the registry after the manifest's, in path
     order like every other signal. ADR 0031 ordering applies, and the registry hash
     covers them.
   - They are `input`s with no device. Their single writer is the service thread
     `svc/metrics`, which writes every signal once per second.
   - The slot instant is the write time, and the quality is `good`. A source that cannot
     be read (no thermal zone) writes `bad(not_connected)`.

2. **The list** (scalar only; units are the canonical ones; counters are `u64` and never
   reset within a daemon's life):

   | Path | Type | Unit | Source |
   |---|---|---|---|
   | `joker.group.<g>.ticks`, `.overruns` | u64 | — | `GroupStats` |
   | `joker.group.<g>.jitter_p99`, `.jitter_max`, `.duration_p99`, `.duration_max` | f64 | s | the HDR histograms |
   | `joker.device.<d>.link` | enum `link_state` | — | device link |
   | `joker.device.<d>.timeouts`, `.reconnects`, `.dropped_samples`, `.invalid_samples` | u64 | — | device counters |
   | `joker.output.<s>.accepted`, `.clamped`, `.rejected`, `.suppressed`, `.tripped` | u64 | — | `EnvelopeResults` |
   | `joker.arbiter.manual_active`, `joker.arbiter.leases_expiring` | u64 | — | manual table, output status |
   | `joker.system.temp` | f64 | K (canonical; shown in °C) | thermal zone 0 |
   | `joker.system.throttled` | bool | — | `vcgencmd`-free: `/sys/devices/platform/soc/soc:firmware/get_throttled` when present |
   | `joker.system.memory_used`, `joker.system.disk_free` | u64 | B | `/proc/meminfo`, `statvfs` of the state directory |
   | `joker.node.boot_count` | u64 | — | `state/boot_count` |
   | `joker.runtime.dropped` | u64 | — | the sum of every internal drop counter (`decisions`, `results`, `trace`, `alarm_events`, `method_results`, `reset_results`, `trip_requests`, logs) |

   The per-queue drop counters also go into `GET /v1/metrics` as `runtime.dropped`, an
   object keyed by queue name. This is the only change to `Metrics`: OpenAPI and
   ref/09 §5.2 change in the same commit. It closes invariant 7's "every drop counted"
   for the internal queues.

3. **`leases_expiring`.**
   - The output status publishes the active level's `lease_until` and the writer's lease
     length beside the value, as two more atomics in the seqlock.
   - A lease is expiring when it is active, is not a manual level, and has less than half
     its length left. Its writer has missed at least half a lease of renewals, and a
     healthy writer, which renews every tick, never qualifies.
   - The gauge counts the outputs with an expiring lease. Nothing is added to the tick
     beyond the two stores.

4. **Measured WCET per task.**
   - The cycle stamps each task's run: two clock reads per task per tick.
   - The stamps feed a per-task allocation-free histogram of the same kind as the group
     jitter.
   - `GET /v1/groups` and `GET /v1/groups/{group}` (normative in ref/09 §5.2, not served
     yet) return `tasks[].wcet_measured_p999` and the rest of `Group`.

5. **Recording and alarms.**
   - The signals are recorded only when `[telemetry.record]` selects them, like any
     signal.
   - A manifest alarm may read them, for example
     `when = "joker.system.temp > 80 °C"`. Its group latches them like any input.
   - Their 1 s cadence is declared, so `max_age` defaults to 3 s.

### Rest of AUR-271

- **Drivers.** The latency percentiles, `bytes_rx`/`bytes_tx` and `coalescing` are
  reported where the driver counts them. Modbus and i2c count bytes; the others leave
  the field out (ADR 0071 point 5: no fabricated zero).
- **Apps.** `traps` and `restarts` come from the app host's counters. `memory` is the
  sum of the linear memories of the instances.
- **Store.** `queue`, `dropped` and `disk_used` come from the historian.
- **Dashboards.**
  - `dist/grafana/joker-node.json` has rows for cycle, drivers, envelope, arbiter, apps,
    store and system, on ADR 0071's names.
  - Proof: Prometheus scrapes the bench Pi. The socket is forwarded over SSH to a local
    `socat` TCP bridge, so no plaintext port opens on the node (ADR 0071 point 1). The
    dashboard is rendered from that data.

### Tests

- The registry holds the runtime signals after the manifest's, in path order, and the
  plan hash is stable.
- `svc/metrics` writes them; `GET /v1/values?sel=joker.**` reads them on the bench Pi.
- The allocation guard still passes with the two task stamps.
- `leases_expiring` counts a writer that stopped renewing and leaves a healthy writer
  out.
- A manifest alarm on `joker.group.main.overruns > 0` fires.

---

## Part C — node operation commands (AUR-259)

Everything here is already normative in ref/09 §9.1 and §5.x, except `jk why`.

1. **`jk logs --follow`**: the `logs` topic of `/v1/ws` (`LogEntry` events, filters
   `source`, `app` and `level`), fed by the log ring's sequence numbers. `jk app logs
   <app>` is `jk logs --app <app>`.
2. **`jk app start|stop|restart <app>`**: `POST /v1/apps/{app}/start|stop|restart`.
   - **stop**: the app's tasks skip their ticks, its leases expire on their own (the
     outputs follow the lease), and the state is volatile until the next generation or
     startup (ref/09 §5.14).
   - **restart**: recreates the instances and takes the app out of `crashloop`.
   - The cycle reads one per-app atomic flag set by the API. No lock.
3. **`jk trace <signal>`**:
   - `GET /v1/signals/{path}/lineage`, built from the plan (driver, device, bus, point,
     acquisition, readers, writers, owning group, readback, atomic group);
   - then the `signal_trace` topic, sampled from the continuous trace ring, one record per
     tick that touched the signal.
4. **`jk doctor [--rt] [--allow-live]`**:
   - `POST /v1/doctor` starts a `doctor` job, and `GET /v1/doctor` returns the last
     report.
   - The `base` checks cover kernel, permissions, groups, watchdog, clock, disk, device
     fallbacks and J0907.
   - `rt` measures wake-up latency on a pinned `SCHED_FIFO` thread for `rt_duration`.
5. **Banner**: every mutating command prints the node, mode and origin line first; the
   line exists for `status` and is extended to every `M` command of ref/09 §9.1.
6. **`jk why <output>`** (F2, docs/26 §7), ADR 0091:
   - It composes `GET /v1/outputs/{path}` (winning level, occupants, leases, envelope,
     last result) with `GET /v1/decisions?output=<path>&limit=5` and with
     `GET /v1/values` for the signals the output's envelope reads (permissive,
     `requires_good`, range expressions; their list comes from the lineage).
   - It is all on one screen and adds no node API.
   - docs/07 gains the row and ref/09 §9.1 the mapping.
7. **SDK `joker::log` macros** (ref/04 §4.11):
   - A 1 KiB stack buffer, `enabled()` first, then `log.log`.
   - The bindings are generated with borrowed parameters, so the fields go out as
     `&[(&str, &str)]` with no allocation.
   - A panic hook logs `error` before the trap.

### Tests

- Each command maps to its ref/09 §9.1 endpoint and returns its `--json`.
- `jk why pi.led` on the bench explains a manual override and a clamp.
- `jk app stop` makes the app's outputs fall back after their lease; `start` brings them
  back.
- `jk logs --follow` shows a line from an app that logs on the bench Pi.

---

## Stress test (mental)

Each question is answered against the design above.

1. **A group writes another group's output directly.** That proposal was already
   rejected `NotOwner` by `Outputs::commit`. With per-group `Outputs` it is still
   `NotOwner`, because the output is not in this group's configurations. The same record
   goes to the decision log.
2. **Two groups bind outputs of the same device.** Each box has one writer group, and
   the device's io task reads every box. `CommitRx` compares each box's own `seq`, so two
   `CommitTx` counters never collide. A forced box is handled per box.
3. **An alarm in a 10 ms group trips an output of a 1 s group, active for 30 ms.** The
   edge push lands in the 1 s group's trip queue, and its next tick trips the output.
   Without the edge, the level read would have missed it. Both are needed.
4. **A reset while the alarm is still active, cross-group.** The reset clears the trip
   in the owner's tick. On the next owner tick the level read sees `active` and
   re-trips: the documented behavior.
5. **A cross-group trip under a full queue.** The drop is counted, and the level read
   still trips on the next owner tick if the alarm is still active. A 30 ms activation
   with a full queue is lost and counted: invariant 7 holds, and the case is written in
   ADR 0089.
6. **Ack of an alarm, routed per group.** An ack used to be drained by whichever group
   ticked first, against a shared `Alarms`. With per-group `Alarms` the command must
   reach the owner, or the ack does nothing. That is why the queue is per group.
7. **Replay of a trace recorded before this change.** Single-thread mode keeps the same
   order, but a cross-group alarm trip now lands on the owner's next tick, not
   immediately. Today's code already applies it "at the next commit of the owner
   group", so recorded traces replay the same. The new level read is the only addition,
   and it only re-trips an output that is already tripped.
8. **Priority inversion.** No locks are shared between group threads on the tick path,
   only queues and seqlocks, where readers never wait. A low-priority group holding a
   seqlock write is a write of a few words.
9. **One CPU in `rt.cpus` with several FIFO threads.** FIFO at distinct priorities on
   one core is exactly rate-monotonic scheduling, which is what admission (M23) checked.
   A group that never yields starves the lower ones, and the supervisor sees them stale
   and forces them safe, which is right. The supervisor is at max priority. It is not
   pinned to the rt CPU, so it still runs.
10. **`max_ticks` in threaded tests.** The claim on an atomic counter gives exactly
    `max_ticks` ticks in total. Tests that need an exact interleaving use the virtual
    pacer.
11. **Decision log order across groups.** Records interleave by time of push. Each
    record carries its own tick and instant; the log never promised a global order
    across groups (docs/02 §6.3 declares no consistency between groups).
12. **Memory.** Each group thread has its own `Classes`/`Alarms` arenas sized for every
    alarm and derived signal. That is a few kB per group on the bench, and the ADR 0087
    RSS figures leave room.
13. **`svc/metrics` writing slots while groups latch them.** These are single-writer
    seqlock slots, and a reader never waits. The writer is one thread, not the cycle, so
    nothing on the tick path changes.
14. **Registry growth.** The bench has 2 groups, 2 devices and 3 outputs, which gives
    about 40 runtime signals: under 2 kB of slots. On a 200-output node it is about
    1,000 more slots, 32 kB.
15. **Old generations and the registry hash.** Adding the runtime signals changes the
    registry hash of the same manifest. The generation store keys on the manifest hash,
    and a rollback rebuilds the plan with the running runtime, so this is consistent.
    The hash is per runtime version, which ADR 0031 allows.
16. **`leases_expiring` with the default lease (3 periods) and a writer that ticks every
    period.** After each renewal, remaining ≈ 3T, and before the next it is ≥ 2T, which
    is above 1.5T, so it is not counted. A writer that misses two ticks drops below 1.5T
    and is counted. That is right.

Verdict: the design holds. Rulings are marked as decisions above; none needs the owner
before building.
