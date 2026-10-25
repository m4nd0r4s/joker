# 02 — Signal model

The signal is the only abstraction that apps see. Everything Joker touches must fit in it, or in one of its three variants (block, stream, event), or in an action. Document 09, attack 1, tests where this abstraction lets detail slip through.

This document explains the model. The normative semantics (quality, expressions, arbiter, envelope) is in document 16; the representation of values, quality and results in each interface (binary, JSON, CBOR) is in `ref/00` §4; the manifest keys are in `ref/01`.

---

## 1. The four forms of data, and actions

| Form | Semantics | Example | Where it lives |
|---|---|---|---|
| **scalar** | last value | `meter.p_total`, `di.door_open` | 32 B slot + process image |
| **block** (M7) | sequence sampled at a fixed rate | `acc1.x` at 25.6 kS/s | per-signal block ring |
| **stream** (M4) | bulky frames, latest or queue | `cam0.frames` | pool of shared buffers |
| **event** | discrete records, queue, all delivered | `vision.detections`, `plc.fault_event` | per-consumer queue, in the historian |
| **action** (M19) | command with arguments and a result | `pcs.reset_fault()`, `opc.method("Calibrate")` | request/response through the driver |

Rule of choice: if what matters is *the value now*, it is a scalar. If what matters is *each occurrence*, it is an event. If it is a *waveform*, it is a block. If it is *large*, it is a stream. If it is *doing something* and not *setting a value*, it is an action.

---

## 2. Scalar types

| Type | Notes |
|---|---|
| `bool` | |
| `i64`, `u64` | drivers convert from i16/u16/i32/u32 with scaling |
| `f64` | default type of physical quantities |
| `f32` | to save historian and uplink space; converted to f64 on read |
| `enum<Name>` | named values declared in the profile or in the manifest (`Mode = { off = 0, standby = 1, run = 2, fault = 3 }`) |
| `bits<N>` | status word with named bits (`status.bit(3)` or `status.alarm_overtemp`) |
| `string` | ≤ 64 bytes UTF-8 (serial numbers, textual states) |
| `bytes` | ≤ 64 bytes |

`string` and `bytes` values live in a wide slot (document 01, §3.2; ADR 0062). Expressions cannot read them; `requires_good` can, because it reads only the quality.

Device signals have no `struct`. Related quantities share a path prefix (`inv1.ac.p`, `inv1.ac.q`, `inv1.ac.f`), and the coherence between them is given by **acquisition coherence** (§6.2). The only exception is **internal signals of apps** (`Internal<T>`, document 04, §3.4): they may be records of up to 64 bytes with scalars only, stored in a wide slot with the same seqlock protocol.

Events and action arguments use **records** with scalar fields, `string` up to 1 KiB and bounded lists (`list<T, 64>`). The schema is declared in the app code (`joker:meta`), in the profile or in the class contract. In binary, a record is deterministic CBOR with integer keys equal to the field position; in JSON, it carries the names (`ref/00` §4.1).

---

## 3. Units

Every numeric signal has a unit. Every numeric port of an app has one too. The binding between the two is checked by `jk check`.

- **Syntax:** a subset of UCUM with the usual industry symbols (`W`, `kW`, `kWh`, `V`, `A`, `Hz`, `var`, `°C`, `K`, `bar`, `m3/h`, `%`, `1`, `gn`…). The normative v1 catalog, with prefixes and quantities, is in document 16, §5.
- **Dimension:** every unit reduces to a vector of SI dimensions plus a factor and an offset. `W` and `kW` have the same dimension, `W` and `Wh` do not.
- **Conversion:** same dimension → the runtime converts (factor, and offset for `°C`↔`K`). Different dimension → error `J0107`.
- **Special cases:**
  - `%` is dimensionless but does **not** bind automatically to `1`. You must declare `bind.x = { signal = "…", as = "fraction" }`, because the 0–1 versus 0–100 confusion is a classic bug;
  - `var` and `W` have the same physical dimension but are different quantities: Joker marks them with distinct *quantity kinds* (`active_power`, `reactive_power`, `apparent_power`), and binding them to each other is `J0108`;
  - rates: `kW/s` is the unit of `rate` for an output in `kW`. `jk check` verifies the coherence.
- **Literals with a unit** in the manifest and in expressions: `"125 kW"`, `"49.8 Hz"`, `"20 kW/s"`, `"2s"`, `"5 %"`.

---

## 4. Quality

Inherited from OPC UA and simplified to three levels plus a reason:

| Code | Reasons |
|---|---|
| `Good` | `None`, `Overridden` (value forced in simulation/test), `LocalOverride` (the equipment is in local mode) |
| `Uncertain` | `Stale`, `NotConfirmed` (diverging readback), `Substituted` (replaced by a declared fallback), `OutOfDistribution` (ML), `SensorCal` (under calibration), `ClockUnsynced`, `Initial` (no value yet since startup), `Frozen` (identical value for `stuck_detect`), `PartialWrite` (atomic group partially written) |
| `Bad` | `CommFailure`, `DeviceFault`, `OutOfRange` (declared physical limits), `ConfigError`, `Stale`, `NotConnected`, `NaN`, `Tripped`, `Deadline` (pipeline or model past its deadline) |

**Frozen sensor.** A broken transmitter or a stalled intermediate PLC can keep returning the same value with `Good`. `stuck_detect = "30m"` on a signal (or in the profile, for quantities that always vary, such as frequency) marks `Uncertain(Frozen)` when the value stays bit-for-bit equal for that time.

**The normative access table** (`get`, `get_uncertain`, `get_or`, `sample`) and the quality rules in expressions are in document 16, §3. The numeric codes and the lowercase names used in JSON and TOML (`uncertain`, `comm_failure`) are in `ref/00` §4.3.

**Invariant 3 of `AGENTS.md`:** a `Bad` sample does not turn into a number. In the SDK, `input.get()` returns `Result<T, QualityError>`, and `input.sample()` returns the full sample for those who want to decide for themselves. `input.get_or(v)` exists and records in the trace that the read was substituted; it does not change the quality of the app's outputs (document 16, §3.1).

**Propagation in expressions:**

| Operation | Rule |
|---|---|
| arithmetic and comparison | worst quality of the operands |
| `a && b`, `a \|\| b` | short-circuit with quality, **symmetric**: `false && Bad` and `Bad && false` give `false (Good)`; `true \|\| Bad` and `Bad \|\| true` give `true (Good)`; in the other cases, the worst. Operands have no effects, and so the side does not matter |
| `any(sel)`, `all(sel)` | the same short-circuit, generalized: a `true (Good)` element decides `any`, a `false (Good)` decides `all`; otherwise, the worst |
| `count`, `sum`, `maxs`, `mins` | worst quality of the elements |
| permissive (`interlock`) | a `Bad` or `Uncertain` result ⇒ **not permitted**, unless `permit_uncertain = true` |
| alarm (`when`) | `Uncertain` evaluates normally; `Bad` ⇒ the alarm goes to state `Unknown` (16 §3.2) |
| window functions | the worst quality `q` such that the samples with quality equal to or worse than `q` cover ≥ 10% of the window duration (16 §3.2) |

---

## 5. Time and sequence

Every sample carries:

- `ts_src`: **UTC** timestamp of the remote device, if the protocol provides it (OPC UA SourceTimestamp, BACnet, MQTT with timestamp);
- `ts_hw`: **monotonic** timestamp of local hardware capture, given by the kernel (GPIO edge in chardev v2, V4L2 buffer, translated EtherCAT DC time). It is the right base for `write_at` (1.1), because it is on the same clock as scheduling. On a signal that comes from an MCU over Joker Link, it is the MCU's `ts_hw` converted to the node's monotonic clock by the `TIME` estimate, with the "link" domain flag (document 01, §3.2; `ref/07` §8.3);
- `ts_acq`: **monotonic** instant at which Joker received it, with the translation to UTC of document 01, §18;
- `seq`: per-signal monotonic counter, which increases with each new sample (even if the value does not change).

`cx.now()` in a cycle task is the instant of the tick's **latch**, and not that of the call. That is why two tasks of the same tick see the same "now", and replay is exact.

---

## 6. Read semantics

### 6.1 Cycle

All tasks of a tick read from the **latch snapshot**: reading the same signal twice in the same tick gives the same sample. Signals written by the group itself show the value **applied in the previous tick**, and not the one proposed in this tick. To read the proposed one, use `output.pending()`.

### 6.2 Acquisition coherence

Signals read by the same driver request (a `read holding registers` that covers `ac.p`, `ac.q` and `ac.f`, for example) receive the same **acquisition id**. `snapshot([…])` and the latch guarantee that, if two signals have the same acquisition id at the time of the read, they come from the same request. Two signals from different requests are never guaranteed to be simultaneous. If that matters, the profile should join them into a single request (document 05, §4).

### 6.3 Between groups

A group reads from another group the last applied value **as of its latch**. Since the periods are different, there is no alignment. If a 10 ms task and a 100 ms task need a value that is coherent between them, one publishes an internal signal and the other reads it, knowing that it may be up to one period late.

### 6.4 Async

Async tasks read the current slot, and `snapshot([…])` gives a coherent read of several slots (all from the same seqlock "epoch"). There is no latch: two consecutive reads may differ.

---

## 7. Write semantics

### 7.1 Lifecycle of a proposal

```
set(v) ──▶ group staging ──▶ arbiter ──▶ envelope ──▶ commit ──▶ driver queue ──▶ equipment
                                │            │                                    │
                                ▼            ▼                                    ▼
                           Overridden   Clamped/Rejected/                       readback (optional)
                                        Suppressed/Tripped                      → NotConfirmed
```

`WriteOutcome`, returned in the tick after the cycle (`output.last_outcome()`) and immediately in an async ticket:

| Outcome | Meaning |
|---|---|
| `Accepted` | applied as proposed |
| `Clamped(Range)`, `Clamped(Rate)` | applied with the value limited by the range or by the rate; carries the applied value. They are two distinct outcomes (`clamped_range` and `clamped_rate`, `ref/00` §4.4) |
| `Suppressed` | within the deadband or the minimum interval; nothing was sent |
| `Overridden(level)` | the proposal occupies its level, but a higher level is active |
| `Deferred` | (async) waiting for the next tick of the owning group |
| `Shadowed` | the app is in shadow mode; recorded, not applied |
| `Rejected(reason)` | `WrongType`, `Interlock`, `Quality`, `Range`, `AtomicGroup`, `NotOwner`, `ClockUnsynced`, `Budget` |
| `Tripped` | the output tripped because of this proposal |

The numeric values and the JSON names (`{"outcome": "clamped_rate", "applied": 1.0}`) are in `ref/00` §4.4. `WrongType` at runtime means a NaN, infinite proposal or one with a nonexistent enum variant (16, E10). On an output of an MCU, the outcome comes from the MCU's arbiter and arrives through `OUTCOMES`, with one or two ticks of delay (16, A12).

### 7.2 Scheduled writes (M6, **1.1**)

**Reserved for 1.1** (D5): the keys `schedule_depth`, `timed`, `on_late` and `late_margin` exist in the schema, and using them on a 1.0 runtime is `J0290`; `write-at` is `@unstable` in the WIT (`ref/04`); the API answers `J-E1017` to a request with `at`. The design stays here so that 1.1 is only a minor change.

`output.write_at(v, t)` schedules a write for the instant `t` (monotonic, usually derived from the `ts_hw` of an input: "400 ms after the photocell edge").

- The queue of scheduled writes per output is bounded (`schedule_depth`, by default 0: scheduled writes disabled; you declare a value, for example 16, to enable them) and ordered by time.
- **Precision:** by default, the write is applied in the first tick of the owning group with `t_tick ≥ t`, which gives a precision of one period. With `timed = true` and a driver with timed output (GPIO via the `rt/drv` thread with `timerfd`, timed hardware outputs), the precision is that of the driver (target ±100 µs on GPIO with PREEMPT_RT).
- **`on_late`** (required when `schedule_depth > 0`, `J0206`): if the proposal arrives after `t − margin`, whatever is declared is done: `apply_now`, `drop`, or `apply(v_fallback)`. There is no default for the same reason there is no default `safe`: the safe side depends on the process (document 08, round 2).
- The envelope is evaluated **at the instant of application**, and not at that of scheduling.

### 7.3 Atomic groups and order (M20)

Some equipment requires several coherent writes: mode + setpoint, or the "high value" and "low value" registers of a u32, or a "commit" after the parameters. The manifest declares:

```toml
[atomic.pcs_cmd]
outputs = ["pcs.mode", "pcs.p_set", "pcs.q_set"]
order   = ["pcs.mode", "pcs.p_set", "pcs.q_set"]   # order on the bus
commit  = { signal = "pcs.apply", value = 1 }      # optional: final confirmation write
```

- In the envelope, either all pass or none passes (step 7). With `on_rate = "proportional"`, if the rate limits one of the outputs to a fraction of the requested step, all those of the group are limited to the same fraction (this preserves relations such as the curvature of a differential vehicle; document 08, round 4).
- In the driver, the group's writes are sent in the declared order, in the same request when the protocol allows it (contiguous Modbus FC16), and with no other writes of the same device interleaved.
- **There is no rollback in the equipment.** If the third write fails on the bus, the first two are already there. The group becomes `Uncertain(PartialWrite)`, with an alarm, and the next proposal repeats the whole group. Document 12, §5, declares this boundary.

---

## 8. Actions (M19)

Not everything is setting a value. Fault reset, calibration start, OPC UA methods, CANopen SDOs, re-arming of protections and firmware update of a field device are **commands**.

```toml
[devices.pcs.actions.reset_fault]
permit  = "pcs.state == PcsState.fault"
role    = "operator"             # who may (an app with declared permission, or a human)
timeout = "5s"
```

- The **implementation** (which registers are written, in what order) belongs to the device profile (`ref/03` §2.2) or to the class contract; the manifest only configures the permissive, the role and the timeout (`ref/01` §5.8.2).
- An action has typed arguments, a result (`Ok(record)` or `Err(reason)`) and a timeout.
- **It is not arbitrated**, because it has no persistent state that can be contested. It is **serialized per device** and subject to a permissive.
- **Idempotency:** each invocation carries a key. A reinvocation with the same key within `dedup_window` returns the previous result instead of repeating the command.
- **Long actions** (firmware, calibration) return an `ActionHandle` with progress events and cancellation (a cancellation ends with the error `cancelled`, `ref/04`).
- Only **async** tasks invoke actions, because an action waits for a response and the cycle does not wait. A cycle task that needs to trigger an action publishes an internal event, and an async task executes it.

---

## 9. Paths

```
path      := segment ("." segment)*
segment   := [a-z][a-z0-9_]*          (max. 32 characters; max. 8 segments)
```

| Prefix | Owner |
|---|---|
| `<device>.…` | driver of the device declared in `[devices.<device>]` |
| `app.<app>.…` | published (derived) and internal signals of an app |
| `calc.<name>` | signals derived by an expression in the manifest |
| `bridge.<bridge>.…` | values received by a bridge (e.g. `bridge.ros.cmd_v`), with their own `max_age` |
| `<instance>.<member>` | aliases of the members of a class instance (document 16, P3) |
| pipeline `to` paths | declared by the pipeline (`vision.detections`, `p1.features`); unique on the site |
| `alarm.<name>` | state of an alarm |
| `joker.…` | the runtime itself (reserved) |

Selectors in the manifest accept `*` for one segment and `**` for several: `"inv*.ac.p"`, `"meter.**"`. `**` is always a whole segment (`"alarm.**"`, and not `"**alarm.**"`). The full grammar is in `ref/01` §3.9; the reserved names and first segments, in `ref/00` §2.

---

## 10. Expressions

A small language, typed, with units, total (no loops or recursion) and evaluated by the core. It is used in permissives, alarms, derived signals (`calc.*`), recording triggers and reset conditions.

```
expr    := or
or      := and ("||" and)*
and     := not ("&&" not)*
not     := "!" not | cmp
cmp     := sum (("=="|"!="|"<"|"<="|">"|">=") sum)?
sum     := prod (("+"|"-") prod)*
prod    := unary (("*"|"/") unary)*
unary   := "-" unary | atom
atom    := literal | path | call | "(" expr ")" | "if" expr "then" expr "else" expr
args    := expr | selector                  (selectors with * and ** only as an argument of aggregation functions)
literal := number unit? | "true" | "false" | Enum.variant | duration
call    := name "(" (expr ("," expr)*)? ")"
```

**Functions:**

| Function | Kind | State |
|---|---|---|
| `abs`, `min`, `max`, `clamp`, `sqrt`, `pow` | numeric | — |
| `good(x)`, `quality(x)`, `age(x)` | meta | — |
| `avg(x, 10s)`, `minw(x, 1m)`, `maxw(x, 1m)`, `rate(x, 5s)` | window | bounded ring; the maximum window is declared (`≤ 15m`) |
| `held(cond, 5s)` | the condition has held for ≥ 5 s | 1 timer |
| `rose(cond)`, `fell(cond)` | edges | 1 bit |
| `hyst(x, lo, hi)` | hysteresis | 1 bit |
| `bit(x, n)` | bits | — |
| `own(x)` | the caller's last proposal, at its level, for the `out` `x` | only in class methods (document 17); outside them, `J0306` |
| `any(sel)`, `all(sel)`, `count(sel)`, `sum(sel)`, `maxs(sel)`, `mins(sel)` | aggregation over a selector with `*`/`**` (`any(alarm.p1_*.active)`), resolved at `jk check` to the fixed list of signals | — |

**Integers.** Arithmetic is done in `f64`; comparisons between two integers of the same signal are exact (16 §4). **`own(x)`** with no previous proposal from the caller returns the applied value of `x` (16 §4).

**Typical compilation errors:** incompatible dimension (`meter.p > 50 Hz`, `J0301`), window above the limit (`J0305`), unknown signal with a suggestion (`J0302`), enum without the variant (`J0303`).

**Cost:** stack bytecode over `SignalId`, with no allocation, with the window state preallocated. Target: ≤ 50 ns per node.

---

## 11. Alarms (M9)

An alarm is a condition with a lifecycle, along the simplified lines of ISA-18.2:

```
            condition true (after delay_on)                 ack
 Normal ─────────────────────────────────▶ Active·Unacked ──────▶ Active·Acked
  ▲  ▲                                            │                     │
  │  │                                            │ condition false     │ condition false
  │  │                   ack                      ▼ (after delay_off)   │ (after delay_off)
  │  └────────────────────────────────────── RTN·Unacked                │
  └─────────────────────────────────────────────────────────────────────┘
```

A condition that becomes true again in `RTN·Unacked` goes back to `Active·Unacked`.

```toml
[alarms.grid_overfreq]
when       = "meter.f > 50.2 Hz"
delay_on   = "2s"
delay_off  = "5s"
severity   = "high"            # low | medium | high | critical
suppress_when = "site.mode == SiteMode.maintenance"
message    = "Grid over-frequency: {meter.f}"
actions    = { trip = ["pcs.p_set"] }   # optional: trip outputs
```

- **Shelving:** an operator may shelve an alarm temporarily (`jk alarm shelve grid_overfreq 2h "maintenance"`), with a required TTL bounded by `max_shelve` (default 24 h, `ref/01` §5.15).
- **Suppression by design** (`suppress_when`): during maintenance or at startup, the operator is not flooded.
- Each alarm is at the same time **two signals**, `alarm.<name>.state` (`enum` with the states of the diagram, plus `Unknown`) and `alarm.<name>.active` (`bool`, true in both `Active·*` states), and a series of **events** (transitions), recorded and sent with the `alarms` priority.
- Apps may also declare alarms (`#[alarm]`, document 04), subject to the same rules.
- **Restarts:** the state of an alarm, its pending acknowledgement and its shelve survive a restart of `jokerd` and a generation switch; the transitions and operator actions are kept in the alarm journal of the historian (ADR 0082).

---

## 12. Classes and instances (M24)

On top of signals there is an optional layer of **equipment classes**: versioned contracts (`led@1`, `pump@1`, `inverter.pv@2`) that group members (inputs, outputs, parameters, actions, events), methods and queries, and **instances** that bind those members to concrete signals. Everything compiles to the signals of this document: the semantics of read, write, quality and time is exactly the same. The detail is in document 14.
