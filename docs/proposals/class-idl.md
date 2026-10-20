# Proposal — The class IDL (`.jkc`)

**Status:** incorporated into **document 17** (ADR 0018). This file remains as a record of the reasoning and of the 15 stress cases (§10). Where it diverges from document 17, **17 prevails** (the syntax was corrected in the readiness round, document 18).

The goal is to answer three questions: what exactly each thing declared in a class is (and why `method` and `action` are different things); how each one runs at runtime; and where the IDL breaks when real equipment is thrown at it.

---

## 1. What the IDL is, and what it is not

A `.jkc` class describes **the contract of an equipment type for programs**. It is read by:

- the `jkc` compiler (inside `jk`), which validates and generates the types for Rust, TypeScript and Python;
- `jk check`, which checks bindings, capabilities, units and atomic groups;
- the core, which executes the methods and the queries;
- the registry, the HMI, the OPC UA bridge and the CLI, which show the instances and let you call methods and actions.

The IDL does **not** have: addresses or protocols (those belong to profiles), safety (that belongs to the manifest: `safe_hint` is only a suggestion), loops, recursion, I/O or state of its own (those belong to active implementations). Everything written in a class must be analyzable without executing code.

---

## 2. The seven things a class declares

This is the central part. Each kind of member corresponds to **a different runtime behavior**, and choosing the wrong one is the most expensive modeling mistake.

| Declaration | What it represents | Runtime semantics | Who produces it | Who consumes it | Where it can be used |
|---|---|---|---|---|---|
| `in` | a quantity the equipment **has now** | last value with quality and time; snapshot in the cycle | the equipment (driver) or the implementation | apps, queries, alarms | cycle and async |
| `out` | a **desired state** someone imposes on the equipment | level held by a lease; arbiter; envelope; `safe` when nobody holds it | apps (through methods or directly) | the equipment | cycle and async |
| `param` | a **characteristic** of the instance that hardly changes | read at startup and on request; no poll in a cycle | the profile, the manifest or the equipment | apps, queries | cycle and async |
| `event` | something that **happened**, once | queue; each occurrence delivered to each consumer | the equipment or the implementation | apps, historian, alarms | async |
| `action` | a **request for the equipment to do something**, once | request/response, with timeout, idempotency, result or error; not arbitrated | apps, CLI, HMI, bridges | the equipment (driver) or the implementation | **async only** |
| `method` | **sugar** for changing one or more `out` coherently | expanded by the core into proposals on the `out`; no response of its own | apps, CLI, HMI, bridges | the core | cycle and async |
| `query` | a value **derived** from the other members | expression evaluated by the core on each tick of whoever consumes it | the core | apps, alarms | cycle and async |

### 2.1 `method` versus `action`: the difference that matters

The best image is this: **a method turns a knob; an action rings a bell.**

| | `method` (`pump.start()`) | `action` (`pump.reset()`) |
|---|---|---|
| What changes | a **desired state** (`run_cmd = true`) that stays in force | nothing stays in force: **something happens** in the equipment |
| If the caller disappears | the lease expires and the `out` goes back to `safe`: the pump stops | nothing: the reset has already been done (or failed) |
| Repeating the call | idempotent: asking for `true` twice is the same as once | dangerous: two resets are two resets, and that is why there is an idempotency key |
| Arbiter | yes: operator vs. optimizer, priorities, overrides | no: there is no state to contend for; it is **serialized** per piece of equipment |
| Envelope | yes: range, rate, interlocks, actuation budget | a **permissive** (`permit`) and a role (`role`) |
| Response | none; the result is seen in the `out` and in the `last_outcome` of the next tick | yes: `Ok(result)` or `Err(reason)`, with a timeout |
| Where it can be called | cycle and async (it is only a proposal) | async only (it waits for a response) |
| Cost | ~ns: it goes in the tick's proposal buffer | a request to the driver: ms to s |
| Example | `start`, `stop`, `set_power(50 kW)`, `toggle`, `open`, `set_mode(auto)` | `reset_fault`, `acknowledge`, `calibrate`, `start_session`, `update_firmware`, `save_config` |

**The decision rule**, for whoever writes the class:

> If, when the caller disappears, the equipment should **go back to a safe state**, it is an `out` (and a `method` on top of it). If what is requested **only makes sense once** and has a result, it is an `action`.

The hard cases are in the attacks of §10 (S2, S6, S8), and they are exactly the ones that force this distinction.

### 2.2 Why a `method` is not an `out` "with a nice name"

Because a method can write **several** `out`, **atomically** and **in order**, with values **computed** from the arguments and the state. `battery.set_power(-30 kW)` writes the mode (`discharge`) and the setpoint (`30 kW`) in the same tick, in the order the equipment requires, and you never see a tick with the new mode and the old setpoint. It is a contract the equipment needs and that whoever uses the class should not have to know.

---

## 3. The language

### 3.1 A complete file

```
// classes/std/pump@1.jkc
package std

/// Pump with start/stop, variable speed and state feedback.
@aligns("CiA 402 when the actuator is a drive")
class pump@1 {

    // ── desired state ────────────────────────────────────────────
    /// Run command.
    out run_cmd:   bool          @safe_hint(false)
    /// Speed reference.
    out speed_set: f64 %         @safe_hint(0 %) @range_hint(0 %, 100 %)

    // ── current state ────────────────────────────────────────────
    /// Run feedback (contactor or drive).
    in  running:   bool
    in  speed:     f64 %
    in  fault:     bool
    in  fault_code: u64          @optional

    // ── characteristics ──────────────────────────────────────────
    param rated_flow: f64 m3/h   @optional

    // ── happenings ───────────────────────────────────────────────
    event tripped { code: u64, at_speed: f64 % }

    // ── requests to the equipment ────────────────────────────────
    /// Resets the pump after a fault. Only allowed with the pump stopped and faulted.
    action reset() -> () @permit_hint(fault && !running) @timeout(5 s)

    // ── sugar over the outs ──────────────────────────────────────
    method start()               { run_cmd = true }
    method stop()                { run_cmd = false; speed_set = 0 % }
    atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
        speed_set = clamp(pct, 0 %, 100 %)
        run_cmd   = pct > 0 %
    }

    // ── derived ──────────────────────────────────────────────────
    query failed_to_start: bool = held(run_cmd && !running, 10 s)
    query failed_to_stop:  bool = held(!run_cmd && running, 30 s)

    // ── optional capabilities ────────────────────────────────────
    capability pressure {
        in pressure_out: f64 bar
        query dry_running: bool = running && held(pressure_out < 0.2 bar, 5 s)
    }

    capability hour_counter {
        in hours: f64 h
    }

    health {
        required running, fault
    }
}
```

### 3.2 Grammar (summary)

```
file        := "package" ident { use } { class | enum | record }
use         := "use" path ("as" ident)?
class       := doc* annot* "class" ident "@" major "{" { item } "}"
item        := member | capability | method | query | health | event | action | nested
member      := doc* ("in" | "out" | "param") ident ":" type unit? annot*
nested      := doc* "member" ident ":" classref ("[" range "]")? annot*     // composition (§8)
capability  := "capability" ident "{" { member | query | method | event | action } "}"
event       := "event" ident record_body
action      := doc* "action" ident "(" params? ")" "->" type annot*
method      := doc* ("atomic")? "method" ident "(" params? ")" ("order" "(" idents ")")? "{" { assign } "}"
assign      := ident "=" expr                         // one out per assignment; at most once per method
query       := doc* "query" ident ":" type unit? "=" expr
health      := "health" "{" "required" idents { "degraded_if_bad" idents } "}"
type        := "bool" | "i64" | "u64" | "f64" | "f32" | "string" | "bytes" | enumref | "bits" "<" n ">"
unit        := unit literal (document 02, §3), optionally with "as" <quantity>
annot       := "@" ident ( "(" args ")" )?
expr        := the expression language of document 02, §10, without selectors
```

Three deliberate syntax choices:

- **The expressions are those of the manifest.** The same parser and the same bytecode: what works in a `calc` works in a `query`.
- **Methods only have assignments.** There is no `if` as a statement (there is `if … then … else` as an expression), no loops, no calls to other methods. A method is a pure function `(arguments, snapshot, own proposals) → assignments`.
- **Annotations are open but namespaced.** Joker's (`@safe_hint`, `@optional`, `@timeout`…) are validated. Third-party ones (`@opcua.node(…)`, `@brick("Pump")`, `@hmi.label("Bomba")`) pass through intact into the metadata, and that is how alignment with standards and the HMI is done.

### 3.3 Types, units and records

```
package std

enum PcsMode { off = 0, standby = 1, charge = 2, discharge = 3 }

record SessionSummary {
    energy:   f64 kWh
    duration: f64 s
    id:       string
}
```

- `enum`s and `record`s belong to the package and are referred to by name (`std.PcsMode`).
- Records only appear in `event` and in `action` arguments or results. An `in`/`out` is always scalar (document 02, §2).
- Units are required on everything that is `f64`/`f32` with physical meaning, and `1` for dimensionless. A number without a unit in a class is `JC0107`.

---

## 4. How it runs: the execution model

### 4.1 A method

```
 app (Wasm, cycle)                 core (same tick)                              equipment
 ─────────────────                 ────────────────                              ─────────
 pump.run_at(60 %)
   └─▶ record in the proposal buffer:   { instance p1, method run_at, args [60 %] }
                                   step 5a: expand methods, in call order
                                     speed_set = clamp(60 %, 0, 100) = 60 %
                                     run_cmd   = 60 % > 0 % = true
                                     → 2 proposals, atomic group, order (speed_set, run_cmd)
                                   step 5b: arbiter (caller's level)
                                   step 6:  envelope (from the manifest)
                                   step 7:  commit ──────────────────────────────▶ contiguous FC16
```

- The method **does not cross the Wasm boundary as a call**: it goes in the same proposal buffer as the packed image (document 09, attack 2). The cost is that of evaluating the expression (~50 ns per node).
- `own(x)` is the last proposal **of the caller, at its arbiter level**, for the `out` `x`, including the previous proposals of the same tick. Two calls to `toggle()` in the same tick cancel each other out, as expected.
- Because the method is expanded by the core, **the same method can be called from anywhere**: `jk call p1.run_at 60% --ttl 10m` in the CLI, an OPC UA method coming from the SCADA, an approved intent. They all go through the same code, at the arbiter level of the caller.
- **Accepted limitation:** within the tick in which it called the method, the app does not know the expanded values (`pending()` of that `out` returns `None`). It knows them in the next tick, with `last_outcome()`.

### 4.2 An action

```
 app (Wasm, async)                          core / driver                           equipment
 ─────────────────                          ─────────────                           ─────────
 pump.reset().await
   └─▶ invoke(p1.reset, args, key) ──▶    permissive (manifest) ✓
                                          caller's role ✓
                                          dedup by the key ✓
                                          device queue (serialized) ──────────────▶ profile steps:
                                                                                     write 40300 = 1
                                                                                     wait 500 ms
                                                                                     write 40300 = 0
   ◀── Ok(()) or Err(Timeout | Denied | Device(msg)) ◀──────────────────────────────
```

- On a **virtual** instance, the action is handled by an async task of the implementation (`#[joker::action] async fn reset(...)`).
- A long action returns an `ActionHandle` with progress and cancellation events (document 02, §8).

### 4.3 A query

Compiled to bytecode and evaluated **at the latch of each group** that consumes it, like a derived signal (document 14, §8, A7). Volatile state: windows ≤ 15 min, edges and timers start again on a restart.

### 4.4 An event

Produced by the driver (an OPC UA event, an alarm word that changes a bit) or by the implementation (`io.state.tripped.emit(…)`). Delivered to each consumer in a bounded queue and recorded if telemetry asks for it.

---

## 5. How a class connects to real things

The class says **what**. The **how** is said by one of three implementers, and it is there (and not in the class) that the differences between pieces of equipment live.

### 5.1 A profile (implicit instance)

> **Obsolete.** The form below (`[implements."std.pump@1"]`, with the members directly in the table) is not the adopted one. The normative form is `[implements."std/pump@1"]` with the members inside `map`, as in the manifest: document 17, §5.1, and `ref/03` §2.2 (finding A11 of `ref/03`). The block remains as a record of the reasoning.

```toml
# profiles/acme/vfd@3.profile.toml
[implements."std.pump@1"]
run_cmd    = "cmd.run"                                       # level → level
speed_set  = { signal = "speed_ref", scale = 0.01 }          # scale adapter
running    = "status.running"
speed      = "speed_act"
fault      = "status.fault"
fault_code = "fault_code"
reset      = "actions.reset_fault"                            # action → profile steps
tripped    = { on_rise = "status.fault", fields = { code = "fault_code", at_speed = "speed_act" } }
capabilities = ["hour_counter"]
hours      = "counters.run_hours"
```

### 5.2 Adapters

Adapters are the piece that absorbs the differences between pieces of equipment without carrying them into the class. They are **declarative** and evaluated by the core, like everything else:

| Adapter | What for | Example |
|---|---|---|
| `scale`, `offset`, `invert` | values and inverted logic | `speed_set = { signal = "ref", scale = 0.01 }` |
| `const` | member the equipment does not have | `speed = "const:100 %"` (fixed-speed pump) |
| `edge` | **a level `out` on equipment that only accepts pulses** | `run_cmd = { edge = { rise = "actions.start", fall = "actions.stop" } }` |
| `level` | an `action` on equipment that only has a register | profile steps (`write 1`, `wait`, `write 0`) |
| `on_rise`/`on_change` | an `event` from a signal | `tripped = { on_rise = "status.fault" }` |
| `enum_map` | enums with different numbers | `mode = { signal = "op_mode", map = { off = 0, standby = 3, charge = 1, discharge = 2 } }` |

### 5.3 The manifest (composite instance) and an app (virtual instance)

They are the same mappings, in the manifest's `[instances.*]` (document 14, §5.2), or implemented in Rust by an app with `#[joker::implements(Pump)]`, which receives the `out` as commands (`io.cmd.*`), publishes the `in` (`io.state.*`), handles the `action` with async tasks and emits the `event`.

---

## 6. What `jkc` generates

```
classes/std/pump@1.jkc
   │ jkc check        parse → resolve packages → types and units → rules (§7)
   ▼
pump@1.contract.json  canonical form, ordered, without comments or formatting
   │                  ├─ contract hash  (members, types, methods, queries) → goes into joker.lock
   │                  └─ documentation hash (docs, HMI annotations)        → does not break compatibility
   ├─▶ Rust    joker-classes-std::Pump   CycleIo/TaskIo handles, enums, records, methods that emit records
   ├─▶ TS      @joker/classes-std        types and methods
   ├─▶ Python  joker.classes.std         likewise
   ├─▶ docs    HTML/Markdown page        for the registry and for the HMI
   └─▶ OPC UA  ObjectType + Methods      for the bridge (M11)
```

**The identity of a class is the hash of the canonical contract, and not the text.** Reformatting, commenting or translating the documentation does not change the contract hash, and no `joker.lock` becomes outdated because of it.

Errors use the prefix `JC` and the `jk` format:

```
error[JC0204]: method assigns to something that is not `out`
  --> classes/std/pump@1.jkc:34:9
   |
34 |         running = true
   |         ^^^^^^^ `running` is `in`: it is the equipment that says it, not the app
   = help: if you meant to request running, it is `run_cmd = true`
```

---

## 7. The rules `jkc check` applies

| Code | Rule |
|---|---|
| `JC0107` | physical quantity without a unit |
| `JC0201` | method assigns to an `out` more than once |
| `JC0204` | method assigns to something that is not an `out` |
| `JC0205` | `atomic` without `order`, or `order` with members that are not assigned |
| `JC0210` | method or query uses a member of a capability without being inside it |
| `JC0301` | expression with incompatible dimensions |
| `JC0310` | queries in a cycle (`a` uses `b` which uses `a`) |
| `JC0320` | query with a window above 15 min |
| `JC0401` | action used in a method (methods do not wait) |
| `JC0402` | action without `@timeout` |
| `JC0501` | composition with depth > 3 |
| `JC0601` | new version incompatible with the previous one without bumping the major (§9) |

---

## 8. Composition and collections

```
class station@1 {
    member pumps: std.pump@1[1..8]
    member tank:  std.tank@1
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
    query pumps_running:   u64  = count(pumps.running)
}
```

- `any`, `all`, `count`, `sum`, `maxs`, `mins` over a member of a collection are the aggregations of document 02, §10, and they are the only way to "iterate" in a class.
- **A method of a composite class cannot iterate** over the members (`stop_all()` is not expressible). It is on purpose: behavior over collections is logic, and it goes to an app or to an active implementation of `station@1`.

---

## 9. Versions

`jk class diff pump@1.jkc@1.2 pump@1.jkc@1.3` classifies each change:

| Change | Classification |
|---|---|
| adding a capability, an `@optional` member, a query, a method, an event, an action | minor |
| changing the documentation or HMI annotations | patch (does not change the contract hash) |
| changing the unit to another of the same dimension (`W` → `kW`) | minor (the runtime converts) |
| changing the dimension, the type, the direction (`in` ↔ `out`), deleting or renaming a member | **major** |
| renaming with `@renamed_from("old")` | minor, with an alias for one major |
| making a member that was optional required | **major** |
| changing the body of a method or of a query | **major**, unless `jk class diff` proves equivalence (rare); rule of thumb: changing the semantics is major |

---

## 10. Stress: 15 cases against the IDL

**S1 — LED.** Three `out` lines, four methods. Holds; it is the trivial case and it must stay trivial.

**S2 — Pump that only accepts start and stop pulses.** Many starters have "start" and "stop" registers that are written with 1 (and the equipment keeps the state), instead of a run bit. If the class had `action start()`, the most important property would be lost: **when the caller disappears, the pump stops**. Decision: the class keeps `out run_cmd` (level) and the **profile** adapts with `edge = { rise = "actions.start", fall = "actions.stop" }`. When the lease expires and `run_cmd` goes to `safe`, the adapter sends the stop pulse. The difference between pieces of equipment stays in the profile, and the safe semantics stays in the class. It is the case that justifies the rule of §2.1.

**S3 — Reset.** It is an `action`: there is no desired state ("being reset" is not something that is held), repetition is dangerous and there is a result (the equipment may refuse). With a lease, a reset would be repeated on every tick. Holds, with the rule.

**S4 — Battery with mode and setpoint, and dynamic limits.** `atomic method set_power(p: f64 kW) order(mode, p_set) { mode = if p < 0 kW then PcsMode.discharge else if p > 0 kW then PcsMode.charge else PcsMode.standby; p_set = abs(p) }`. The mode and the value arrive together. But the charge limit changes with the cell temperature (`in p_max_charge`), and the class cannot impose it, because safety belongs to the site. **Finding:** the manifest envelope needs to accept **dynamic ranges**, with expressions (`range = ["-bess.p_max_discharge", "bess.p_max_charge"]`). The class can suggest the relationship with `@range_hint(-p_max_discharge, p_max_charge)`, and `jk check` shows the suggestion. It is a change requested of document 03 (§12).

**S5 — Five inverter vendors.** Small core (`p_ac`, `p_limit_pct`, `state`) and capabilities (`reactive`, `ramp`, `mppt_detail`). A vendor without percentage limiting, only in W: the profile adapts with `scale` from a `param rated_power`. Holds.

**S6 — Electric vehicle charger.** The case that uses every declaration:

```
class evse@1 {
    in  state: EvseState                   // idle, connected, charging, fault
    in  current: f64 A
    out current_limit: f64 A   @safe_hint(6 A)   // desired state: if the manager dies, 6 A
    event session_ended SessionSummary
    action start_session(id_tag: string) -> SessionId   @timeout(10 s)
    action stop_session(id: SessionId) -> SessionSummary @timeout(10 s)
    method limit(a: f64 A) { current_limit = a }
}
```

The current limit is an `out`, because it must go back to a safe value if the load manager disappears. Starting and ending a session are `action`s, because they happen once, have a response (the id, the summary) and should not be repeated. The end of the session is also an `event`, for whoever did not request it (billing). **Holds, and shows that the three things really are different.**

**S7 — Firmware update.** `action update_firmware(image: ArtifactRef) -> () @timeout(30 min) @long`. A long action with progress. It is not a method, and much less an `out`. Holds.

**S8 — Parameter stored in EEPROM.** A drive has the "default speed" in a parameter that is written to non-volatile memory, with ~100,000 write cycles. If it were an `out`, the keepalives (document 01, §8) and the writes on every change would wear out the EEPROM in weeks. Decision: **persistent equipment configuration is an `action`** (`action set_default_speed(pct: f64 %)`) or a `param`, and never an `out`. (The proposal foresaw a `jk device configure` to write a `param`; **it was not adopted**. In v1, persistent configuration is done through a profile `action`, or through the device `config` that the profile applies at `connect`, `ref/03` §2.2.10. There is no command or endpoint that writes a `param`, `ref/09` §9.) Rule added to §2.1: *if writing causes wear, it is not an `out`*.

**S9 — `toggle()` twice in the same tick.** The expansion is sequential and `own()` sees the previous proposals of the tick: the result is the initial value. It is the expected behavior, and it goes into a conformance case.

**S10 — The SCADA calls a method.** The OPC UA bridge generates one `ObjectType` per class, and each `method` and `action` becomes an OPC UA method. A call to `run_at(60)` coming from the SCADA is expanded by the core **at the arbiter level of the bridge** (document 03, §12). The SCADA cannot do anything that an operator on the CLI could not, and it goes through the same envelope. Holds, and it is the definitive argument against methods in app code.

**S11 — `station.stop_all()`.** It is not expressible (§8). The temptation to add loops to the IDL is strong, and it should be refused: it would be the first step toward a programming language inside the IDL. The way is an active implementation of `station@1`, or an app.

**S12 — A method that needs to read an input.** `method safe_start() { run_cmd = !fault }`: the expression reads an `in`, which is allowed (it reads from the caller's snapshot). But "start only if there is no fault" is **safety**, and it belongs to the manifest's interlock. `jkc` accepts it, and emits a warning (`JW0230: a method conditioned on an `in` looks like an interlock; confirm that the manifest has it too`).

**S13 — Reading an `out`.** What is `p1.run_cmd` for whoever reads it? There are three values: my last proposal, the effective value (whoever won the arbiter) and the applied value (after the envelope). Decision: reading an `out` gives the **applied** one (it is what the equipment received). `p1.run_cmd.mine()` gives my proposal, and `p1.run_cmd.winner()` gives the level that won. In the class's expressions, `run_cmd` is the applied value and `own(run_cmd)` is the caller's.

**S14 — Two versions of the same class on a site.** An old profile implements `pump@1.2` and an app asks for `pump@1.4` with a new capability. `jk check` compares the requested capability with those declared by the instance (`J0602`) and not with the minor version: a `1.2` instance that does not have the capability fails, and one that has it passes. Holds.

**S15 — The IDL grows until it is a language.** It is the underlying risk. Containment: the grammar of §3.2 is closed and fits on one page; any new construct requires an ADR; and the test of whether a proposal belongs in the IDL is always the same: *does the core need to execute or analyze it for whoever is not a Rust app?* If not, it goes to an extension trait or to an active implementation.

---

## 11. Comparison with what already exists

| | Joker `.jkc` | WIT | OPC UA NodeSet | Protobuf/gRPC | IEC 61131 FB |
|---|---|---|---|---|---|
| Units and quantities | yes, checked | no | yes (EUInformation), not checked | no | no |
| Desired state with lease and `safe` | yes (`out`) | no | no | no | partial (outputs) |
| Request/response | yes (`action`) | yes | yes (Methods) | yes | no |
| Events | yes | yes (streams) | yes | streams | no |
| Derived values evaluated by the system | yes (`query`) | no | no | no | yes (code) |
| Optional capabilities | yes | no (world composition) | partial (optional) | no | no |
| Analyzable without executing | yes | yes | yes | yes | no |

None of the existing ones brings together **units + safe desired state + request/response + derived values**. That is what justifies an IDL of its own instead of WIT with annotations: almost everything would need annotating, and the annotations would not be checked.

---

## 12. What this proposal asks of the other documents (if accepted)

1. **Document 14:** replace the TOML blocks of the classes with `.jkc`; add `param`, `event` and the table of §2 as the official definition of the members; the rule of §2.1.
2. **Document 03:** envelope ranges with expressions (S4); ~~`jk device configure` for persistent parameters (S8)~~ (not adopted; see S8).
3. **Document 05:** the adapters of §5.2 in the `implements` sections of the profiles, including `edge` (S2).
4. **Document 07:** `jk class fmt | check | diff | doc | gen`, `jk call <instance>.<method>`.
5. **Document 15:** `classes/**/*.jkc` and generated `*.contract.json`.
6. **Track, part 8:** `status_light@1.jkc`.
7. **ADR 0018:** "one format per kind of thing: TOML for configuration, `.jkc` for contracts, code for behavior".

## 13. How to validate before deciding

- Write the ~12 classes of `std/` (document 14, §10) in `.jkc` and in TOML, side by side.
- Ask two people (one from automation, one from software) to write `valve.motor@1` and `evse@1` with only this proposal.
- Implement `jkc check` for the grammar of §3.2 (estimate: 1–2 weeks, reusing `joker-expr`).
- Accept if: the DX is clearly better; none of the cases S1–S15 needs more than the grammar; and the generated canonical form is identical in both versions (which proves that the surface format is only surface).
