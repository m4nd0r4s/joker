# AUR-274 — rt/supervisor, force_safe, persistent trips/manual levels, hardware watchdog

Status: approved, in implementation.

## 0. Why this is mostly wiring, not new design

Three facts from the codebase reshape this from "design a new safety
mechanism" into "build the supervisor thread that drives mechanisms that
mostly already exist":

1. **`force_safe` is already built and tested** —
   `joker-hal::commit::{CommitBox, ForceSafe, CommitRx}`. `ForceSafe::set(index,
   bool)` is the supervisor's write handle; `CommitRx::take()` already emits a
   `CommittedWrite{kind: ForceSafe}` carrying the output's declared `safe` the
   moment the bit rises, bypassing the group entirely — exactly docs/09 attack
   4(c)'s refinement. Nothing in `jokerd` calls `ForceSafe::new()` yet.
2. **One `rt/cycle` thread handles every group today** (AUR-275, one thread per
   group, is untouched Backlog). This spec builds the supervisor against
   today's single-thread reality; §3 shows the detection math survives the
   later transition to per-group threads unchanged.
3. **`Outputs::trip(signal)` already exists** ("takes effect at the next
   commit") and is unused outside the internal range-eval path — it is the
   exact primitive needed to restore a persisted trip at boot, no new
   `joker-core` surface required.

A fourth fact sets a hard boundary: **there is no real alarm delivery path in
jokerd today** (M9/AUR-288 untouched; `GET /v1/alarms` is hardcoded empty).
Every "raise alarm X" in the specs (critical alarm on stuck group,
`manual_restored` medium alarm) is a **decision-log entry + structured log
line** for now, with a one-line note pointing at AUR-288. This spec does not
fabricate alarm delivery that doesn't exist.

## 1. Scope

**In scope:**
- The `rt/supervisor` thread: hang detection, `force_safe` activation,
  lease-expiry bypass (not a separate mechanism, see §3)
- Hardware watchdog feed (`/dev/watchdog*` per board, `sd_notify
  WATCHDOG=1`) + systemd unit update
- Persistent trips (`state/trips`, new) and manual levels (`state/manual`,
  rewriting the already-shipped A14 code onto the documented persistence
  convention)
- Boot sequence ordering: restore trips + manual levels before the first
  tick, boot counter

**Out of scope (deferred, not dropped):**
- Per-group `force_safe` granularity — needs AUR-275 first
- Real alarm delivery — needs AUR-288/M9
- EtherCAT/piControl bus-level watchdogs (`rt/drv/<bus>` threads don't
  exist)
- Class-instance trips/leases (AUR-407 not started)

## 2. Persistence layer (`state/trips`, `state/manual`)

Both follow the on-disk-layout convention (docs/ref/02 §2.2.4): write to a
temp file under `tmp/`, `fsync` the file, `rename(2)` over the target,
`fsync` the directory.

```
state/trips    — JSON array of {output, since_utc_ns, reason}
state/manual   — JSON array of {output, level, principal, value, reason,
                  since_utc_ns, until_utc_ns, ttl_ns}   (replaces manual.json)
```

New module `jokerd::state_file`:
```rust
pub fn write_atomic(state_dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()>;
pub fn read(state_dir: &Path, name: &str) -> Option<Vec<u8>>; // missing file = None
```

`ManualTable::save`/`load` move onto this helper instead of their current
plain `fs::write`/`fs::read_to_string`. A new `TripTable` (simpler: no TTL,
no arbitration, just tripped output paths + reason + since) uses it too.

**Clock-sync branch (A14, currently missing):** on load, if
`SystemPacer::host_clock_quality()` is not `Synced`, UTC deadlines cannot be
trusted — restore using the **remaining TTL recorded at write time**
(stored alongside `until_utc_ns`), counted from `now_mono_ns` at boot, and
log a `manual_restored` decision-log entry (would be `medium` severity once
alarms exist, §0). Trips have no TTL, so this branch does not apply to them.

## 3. The `rt/supervisor` thread

**Detection — unchanged before and after AUR-275:**

Add `GroupStats.last_tick_mono_ns: AtomicU64`, set at the start of every
tick the cycle thread runs (regardless of which group). The supervisor
polls periodically and, for each group `g`:

```
stale(g) = now_mono_ns() - groups[g].last_tick_mono_ns.load() > 3 * g.period_ns
```

Today, every group shares one thread, so a real hang makes every group go
stale within one tick of each other — "any group stale" is operationally
"the thread is dead," and the response forces every output of every group.
After AUR-275, each group ticks on its own thread, `stale(g)` becomes
genuinely per-group, and the response narrows to just that group's
outputs — the detection formula does not change, only how many groups the
response reaches.

**Response, on any `stale(g)`:**
1. `group_boxes: Vec<Vec<(usize, usize)>>` (device_index, box_index pairs
   per group), precomputed once per generation at `Running::start` from
   `OutputPlan.group` + `commit_bindings`.
2. For today's whole-thread response: iterate every group's box list and
   `ForceSafe::new(&shared.commit_boxes[device]).set(box_index, true)`.
3. Log a decision-log entry (`kind: "supervisor_force_safe"`, group,
   elapsed) — stand-in for the critical alarm (§0).
4. Stop feeding the hardware watchdog (§4).

**Lease expiry "outside the group thread"** is not a second mechanism: it
already happens every tick inside `commit()` (`Cause::Expiry`). When the
group cannot tick at all, it cannot expire its own leases either, so
forcing `safe` directly through the box *is* the "outside the group
thread" expiry path.

**Scheduling:** `SCHED_FIFO` max priority (`nix`, same crate already used
for `mlockall`/`sched_setaffinity`). A failure to raise priority (no
`CAP_SYS_NICE`, running under `cargo test`) is a warning, not fatal, same
convention as `lock_memory()`.

## 4. Hardware watchdog feed

Reuse `BoardPeripherals::watchdog` (already built for `jk board show`): if
the active board declares `[watchdog]`, the supervisor opens `device`
(`/dev/watchdog0`) and writes a keepalive byte every
`min(timeout, max_timeout) / 3`. A failure to open the device is a warning
(most dev machines have none).

`sd_notify WATCHDOG=1`: implemented by hand (no new dependency) — a
`SOCK_DGRAM` Unix socket send of `b"WATCHDOG=1\n"` to `$NOTIFY_SOCKET`. Also
send `READY=1` once, right after the API socket is listening. Switch
`dist/jokerd.service` to `Type=notify` with `WatchdogSec=<N>`.

Both feeds stop the moment `stale(g)` is true for any group — this is what
actually triggers the board-level reset in the fault matrix's "Dead jokerd
→ systemd/hardware watchdog → restart."

## 5. Boot sequence

In `Node::new`, before `restore_manual`:
1. `TripTable::load(state_dir)` → `Vec<SignalId>` resolved against the
   active plan (unknown output dropped, same policy as A14).
2. Inject via a new `RunOptions.initial_trips: Vec<SignalId>`, applied once
   in `cycle::run()` right after `Outputs::new()`, via the existing
   `Outputs::trip(signal)` — mirrors how A8's `inherit` seeds `state.last`.
3. `ManualTable::load` (rewritten onto `state_file`) + `restore_manual`
   (logic unchanged).
4. Boot counter: `state/boot_count` (plain u64, same atomic-write helper),
   incremented once per `Node::new`, surfaced in `GET /v1/info` for now
   (no metrics system yet, AUR-271).

Last confirmed generation and outputs-start-at-safe are already correct
(`Controller::boot()`; `Outputs::new()` always starts at `safe`, A8) —
nothing to build there.

## 6. Testing strategy

- **Hang detection math**: unit test, fake `GroupStats` + virtual clock.
- **force_safe activation**: integration test driving the supervisor's
  response function against a real `Shared`/`commit_boxes`, asserting the
  next `CommitRx::take()` yields `WriteKind::ForceSafe`.
- **Real hang simulation**: a `NativeTasks`-provided task that genuinely
  blocks past `3 * period`, catching the host-level hangs epoch
  interruption does not cover.
- **Boot restore**: the two-`Node`-over-one-`TempDir` pattern already used
  for the A14 restart test, extended with a persisted trip.
- **Watchdog feed / sd_notify**: unit-testable with a fake
  `$NOTIFY_SOCKET` listener in-process; real `/dev/watchdog` stays
  manual/bench-only.

## 7. Delivery order

1. Persistence rewrite: `state_file` helper, `TripTable`, A14 rewrite onto
   it — lowest risk, no new threads.
2. Boot-restore wiring (trips + boot counter) — builds on (1).
3. Supervisor thread + `force_safe` wiring + decision-log stand-in for
   alarms — the core safety piece.
4. Hardware watchdog + `sd_notify` + systemd unit — depends on (3) existing
   to know when to stop feeding.
