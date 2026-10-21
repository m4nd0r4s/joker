# 17 — The class IDL (`.jkc`)

Normative document (ADR 0018). It defines the language in which **equipment classes** (document 14) are written: what each declaration means, how it runs at runtime, how it binds to real equipment, what the compiler generates and how it evolves. It results from the proposal `proposals/class-idl.md`, corrected after the readiness round (document 18).

The rule that decides the format of everything in Joker (ADR 0018): **configuration is TOML; contracts are `.jkc`; behavior is code.**

---

## 1. What the IDL is, and what it is not

A `.jkc` class describes the **contract of an equipment type for programs**. It is read by the `jkc` compiler (inside `jk`), by `jk check`, by the core (which executes methods and queries), by the registry, by the HMI, by the OPC UA bridge and by the CLI.

The IDL has **no** addresses or protocols (those belong to profiles), safety (that belongs to the manifest; `@safe_hint` is only a hint), loops, recursion, I/O or state of its own (those belong to active implementations). Everything in a class is analyzable without executing code.

---

## 2. The seven declarations

| Declaration | What it represents | Runtime semantics | Produced by | Consumed by | Cycle / async |
|---|---|---|---|---|---|
| `in` | a quantity the equipment **has now** | last value with quality and time | equipment or implementation | apps, queries, alarms | both |
| `out` | a **desired state** imposed on the equipment | level held by lease; arbiter; envelope; `safe` with no writer | apps (through methods or directly), CLI, bridges | equipment | both |
| `param` | a **characteristic** of the instance, which hardly changes | read at startup and on request; no cyclic poll | profile, manifest or equipment | apps, queries | both |
| `event` | something that **happened** once | queue; each occurrence delivered to each consumer | equipment or implementation | apps, historian, alarms | async |
| `action` | a **request for the equipment to do something**, once | request/response, timeout, idempotency, result; serialized; not arbitrated | apps, CLI, HMI, bridges | equipment or implementation | **async only** |
| `method` | sugar to change one or more `out` coherently | **expanded by the core** into proposals on the `out` | apps, CLI, HMI, bridges | core | both |
| `query` | a value **derived** from the other members | expression evaluated by the core on each tick of whoever consumes it | core | apps, alarms | both |

### 2.1 `method` versus `action`

A method turns a knob; an action presses a doorbell.

| | `method` (`pump.start()`) | `action` (`pump.reset()`) |
|---|---|---|
| What changes | a desired state that stays in force | nothing stays in force: something happens |
| The caller disappears | lease expires → `safe` | nothing |
| Repeating | idempotent, if the method does not use `own()` (a repeated `toggle()` inverts twice) | dangerous; idempotency key |
| Arbiter / envelope | yes / yes | no / permissive + role |
| Response | in the cycle, none; in async, `await` returns the `WriteOutcome` of the expanded writes, after the commit | `Ok(result)` or `Err(reason)` |
| Cost | ~ns (goes in the proposal buffer) | a request to the driver: ms to s |

**Decision rules:**

1. If, when the caller disappears, the equipment should return to a safe state, it is an `out` (with methods on top).
2. If it only makes sense once and has a result, it is an `action`.
3. If writing has a physical cost (EEPROM, flash, wear), it is an `action` or a `param`, and never an `out`.

---

## 3. The language

### 3.1 A complete file

```
// classes/std/pump.jkc
package std

/// Pump with start/stop and status feedback.
@aligns("CiA 402 when the actuator is a drive")
class pump@1.0.0 {

    /// Run command.
    out run_cmd: bool            @safe_hint(false)

    /// Run feedback (contactor or drive).
    in  running: bool
    in  fault:   bool
    in  fault_code: u64          @optional

    param rated_flow: f64 m3/h   @optional

    event tripped { code: u64 }

    /// Resets after a fault.
    action reset() -> ()         @permit_hint(fault && !running) @timeout(5 s)

    method start()               { run_cmd = true }
    method stop()                { run_cmd = false }

    query failed_to_start: bool = held(run_cmd && !running, 10 s)
    query failed_to_stop:  bool = held(!run_cmd && running, 30 s)

    capability variable_speed {
        out speed_set: f64 %     @safe_hint(0 %) @range_hint(0 %, 100 %)
        in  speed:     f64 %
        atomic method run_at(pct: f64 %) order(speed_set, run_cmd) {
            speed_set = clamp(pct, 0 %, 100 %)
            run_cmd   = pct > 0 %
        }
    }

    capability pressure {
        in pressure_out: f64 bar
        query dry_running: bool = running && held(pressure_out < 0.2 bar, 5 s)
    }

    capability hour_counter {
        in hours: f64 h
    }

    health required(running, fault)
}
```

A fixed-speed pump is an instance **without** the `variable_speed` capability: there are no output members that must be bound to constants.

### 3.2 Grammar

```
file        := "package" pkgpath { use } { class | enum | record }
pkgpath     := ident { "/" ident }                       // std, acme/water
use         := "use" pkgpath "/" ident ("@" major)? ("as" ident)?
class       := doc* annot* "class" ident "@" semver "{" { item } "}"
item        := member | nested | capability | method | query | health | event | action
member      := doc* ("in" | "out" | "param") ident ":" type unit? ("=" literal)? annot*
nested      := doc* "member" ident ":" classref ("[" range "]")? annot*    // class member (§8)
range       := n ".." n                                  // 0 ≤ min ≤ max ≤ 1024
capability  := "capability" ident "{" { member | nested | query | method | event | action } "}"
event       := doc* "event" ident ( record_body | ":" recordref )
action      := doc* "action" ident "(" params? ")" "->" rtype annot*
method      := doc* ("atomic")? "method" ident "(" params? ")" ("order" "(" idents ")")? "{" { stmt } "}"
stmt        := assign | call
assign      := ident "=" expr
call        := ident "." ident "(" ( expr { "," expr } )? ")"   // method of a fixed class member (§4.1)
query       := doc* "query" ident ":" type unit? "=" expr
health      := "health" "required" "(" idents ")"
type        := "bool" | "i64" | "u64" | "f64" | "f32" | "string" | "bytes" | "bits" "<" n ">" | enumref
rtype       := type | recordref | "()" | "list" "<" (type | recordref) "," n ">"
param       := ident ":" (type unit? | recordref | "list" "<" … ">")
classref    := pkgpath "/" ident "@" major
annot       := "@" ident ("." ident)* ( "(" args ")" )?
expr        := the language of document 02, §10 (+ own(x) in methods; + m.x over a fixed class member m;
               + aggregations over collection members)
```

- **Versions:** the declaration has the full version (`pump@1.0.0`); references use the major (`std/pump@1`). `joker.lock` pins the exact version and the contract hash.
- **Implicit package:** the `std` package may be omitted in references: `pump@1` ≡ `std/pump@1`. Any other package is always explicit.
- **Names:** packages with `/` (`std`, `acme/water`); classes may have dots in the name (`hvac.vav`), because the package separator is `/`: `std/hvac.vav@1`.
- **Namespace within a class:** members, arguments and queries share it. An argument with the name of a member is `JC0206`.
- **Reserved names:** `name`, `index`, `health`, `read`, `raw` and `has` are the API of the generated handles in all languages, and must not be names of members, queries, methods, events or actions (`JC0207`). A name that is a reserved word of a target language (`from`, `in`, `class` in Python) is accepted, and that language's generator appends `_` to it (`p.from_()`), with a warning.
- **Method versus member:** a method may have the name of a member (`led@1`: `out on` and `method on()`). In the generated handles, the name gives the **method**, and the member goes in `raw()` (`led.raw().on`, ref/04 §4.8.2); in the CLI, in expressions and in the manifest, `led.on` is the member.
- **Methods:** only assignments to the class's own `out` and calls to methods of **fixed class members** (`compressor.stop()`, §4.1). There is no `if` as a statement (there is `if … then … else` as an expression), no loops, no calls to actions (`JC0401`), and no calls to methods of a collection member (`JC0502`). An assignment to an `out` of a member (`compressor.run_cmd = true`) is `JC0204`: the member is changed through its methods. Each `out` is assigned at most once per method, counting the writes that the calls expand (`JC0201`).
- **Class members** (`member`, §8): fixed (`member tank: local/tank@1`) or collections (`member pumps: std/pump@1[1..8]`). They accept `@optional` and `@requires(<capability>, …)`, the capabilities the bound instance must have (`JC0504` if the member's class does not have them). They may be inside a `capability`. In the class's expressions, `m.x` reads member `x` of fixed member `m`; a collection is read only through aggregations (§8).
- **Annotations:** Joker's own (`@safe_hint`, `@range_hint`, `@permit_hint`, `@role_hint`, `@timeout`, `@optional`, `@requires`, `@long`, `@aligns`, `@renamed_from`, `@deprecated`) are validated. Third-party ones, with a namespace (`@opcua.node(…)`, `@brick("Pump")`, `@hmi.label("Pump")`), pass through intact into the metadata.

### 3.3 Enums and records

```
package std

enum PcsMode { off = 0, standby = 1, charge = 2, discharge = 3 }

record SessionSummary {
    id:       string
    energy:   f64 kWh
    duration: f64 s
}
```

`in`/`out`/`param` are always scalars or enums (document 02, §2). Records and bounded lists (`list<T, N>`, N ≤ 64) appear only in `event` and in arguments or results of `action`.

---

## 4. Execution model

### 4.1 Method

1. The app calls `pump.run_at(60 %)`. The SDK appends to the tick's **proposal buffer** a **method record**: `(instance, method, arguments)`, with the arguments in CBOR (ref/04 §3.3).
2. In step 5a of the tick (document 01, §4), the core expands the records **in call order**: it evaluates the assignments on the tick snapshot and with `own()` = the caller's last proposal at its level, including the earlier ones in the same tick. **With no earlier proposal** (first call, or expired lease), `own(x)` is the applied value of `x` (document 16, §4).
3. The assignments become normal proposals, with an atomic group and order if the method is `atomic`, and go on to the arbiter and the envelope.

Consequences:

- The same method is called from anywhere through the same path: `jk call p1.run_at 60% --ttl 10m`, an OPC UA method coming from the SCADA, an approved intent. The expansion happens on the next tick of the output's owning group, at the caller's arbiter level.
- Two calls to `toggle()` in the same tick cancel each other out.
- On the tick in which it called the method, the app does not know the expanded values (`pending()` of those `out` returns `None`). It knows them on the next tick, with `last_outcome()`, or in async by waiting on the `await`.

**Methods that call members' methods.** A method of a composite class may call a method of a **fixed member**:

```
class compressor@1.0.0 {
    member drive: std/drive@1
    in discharge_temp: f64 °C

    method start(pct: f64 %) { drive.run_at(pct) }
    method stop()            { drive.stop() }
}
```

1. The expansion is that of step 2, recursive: the core resolves `drive` in the composite instance (the instance bound to that member, ref/01 §5.13), evaluates the arguments on the tick snapshot and expands the called method over that instance, **in the same step 5a**. The whole call tree expands at once; there is no tick per level.
2. The expanded writes are proposals **of the writer that called the top-level method**, at its level: `own()` inside `drive.stop()` is that of the caller of `comp1.stop()`. For the arbiter of `vfd1.run_cmd`, there is no difference between calling `comp1.stop()` and calling `vfd1.stop()` directly (document 16, A1 and P6).
3. In an `atomic` method, the writes expanded by the calls enter the same atomic group. The `order` may name the member (`order(drive, bypass)`), and its writes take that position, in the order of the called method.
4. The depth of the calls is bounded by that of the composition (§8): at most 3 classes in a chain, and there is never recursion, because a class cannot contain itself.
5. Calling a method of a collection member (`pumps.stop()`), or iterating, is `JC0502`: behavior over collections goes into an app or into an active implementation. Calling a method the member does not have, or one of a capability the member does not require with `@requires`, is `JC0503`. Calling an action is `JC0401`.
6. **In a virtual instance**, the class members belong to the implementation, and whoever uses the instance does not write to them (document 16, P6). A virtual instance of a class whose methods call members' methods is `J0656` in `jk check`: the call would make the caller a second writer of the members, alongside the implementation. A class meant to be used as virtual writes only to its own `out`, and the implementation decides what to do with the members.

### 4.2 Action

`invoke(instance.action, args, key)` → permissive and role (from the manifest) → deduplication by key → the device's serialized queue → profile steps (or the virtual implementation's `on-action` task) → `Ok`/`Err`. An `@long` action returns an `ActionHandle` with progress events and cancellation.

**Action configuration:** the class gives hints (`@timeout`, `@permit_hint`, `@role_hint`). The manifest decides, in `[devices.<d>.actions.<a>]` (implicit instances) or in `[instances.<i>.actions.<a>]` (composite and virtual), with the keys `permit`, `role` and `timeout`. The manifest always wins, and a class `@timeout` is used only when the manifest does not declare one.

### 4.3 Query

Compiled to the expression bytecode and evaluated at the latch of each group that consumes it. Volatile state (document 16, §3.2).

### 4.4 Event

Produced by the driver, by an `on_rise`/`on_change` adapter of the profile, or by the implementation. Bounded queue per consumer; recorded if telemetry asks for it.

### 4.5 Health

`health required(a, b)`: with one of the required ones not `Good`, the instance is `Failed`; with a member of a declared capability not `Good`, it is `Degraded`; otherwise, `Ok`.

**A composite instance** (with class members, §8) has the **worst** health (`Failed` > `Degraded` > `Ok`) between that of the rule above, over its scalar members, and that of its class members:

| Class member | Counts as |
|---|---|
| required (outside capabilities, without `@optional`) | the health of the bound instance, as is; in a collection, the worst of the elements |
| of a capability the instance declares | at most `Degraded`: a `Failed` of the member gives `Degraded` |
| of a capability the instance does not declare | does not count (it is not bound) |
| `@optional` | only if it is bound, and at most `Degraded` |

The rule is recursive: the health of the bound instance already includes that of its members. The handles' `Degraded(m)` and `Failed(m)` (ref/04 §4.8.2) carry the name of the class member (`compressor`), and `GET /v1/instances/{i}` shows the tree with the health of each node (ref/09 §5.11). `health required(…)` names only scalar members; class members count by the table.

**An implicit instance with several classes.** A profile with several `[implements]` (a VFD that is `pump@1` and `meter.ac3@1`) gives **one** implicit instance per device (ref/03 §2.2.14) that fulfills all those classes. Its health is the **worst** of the healths computed by the rule of each class it fulfills (`Failed` > `Degraded` > `Ok`), each over the members mapped to that class. `jk status --instances` and `GET /v1/instances/{i}` show the aggregated health and, per class, that of each rule, so one can see which of them lowered it. A handle of one of those classes in an app (`io.p.health()`) returns the aggregated health, and not that of its class: it is the same equipment.

---

## 5. Binding to real equipment

### 5.1 Profile (implicit instance)

```toml
# profiles/acme/vfd@3.profile.toml (excerpt)
[implements."std/pump@1"]
capabilities = ["variable_speed", "hour_counter"]
map = { run_cmd = "cmd.run", running = "status.running", fault = "status.fault", fault_code = "fault_code", speed_set = { signal = "speed_ref", scale = 0.01 }, speed = "speed_act", hours = "counters.run_hours", reset = "actions.reset_fault", tripped = { on_rise = "status.fault", fields = { code = "fault_code" } } }
```

### 5.2 Adapters

The values of the `map` are a path, or an adapter. All are declarative and evaluated by the core.

| Adapter | What for | Example |
|---|---|---|
| `scale`, `offset`, `invert` | values and inverted logic | `{ signal = "ref", scale = 0.01 }` |
| `const:` | **only on `in` and `param`**: a member the equipment does not measure (`J0603` on an `out`) | `"const:true"` |
| `edge` | a level `out` on equipment that only accepts pulses | `{ edge = { rise = "actions.start", fall = "actions.stop" } }` |
| action steps | an `action` over registers | profile steps (document 05, §6) |
| `on_rise`, `on_change` | an `event` from a signal | `{ on_rise = "status.fault" }` |
| `enum_map` | enums with different numbers | `{ signal = "op_mode", enum_map = { off = 0, standby = 3, charge = 1, discharge = 2 } }` |

**The `edge` adapter, in detail.** It is what lets a pump with pulse run and stop buttons keep the safe semantics of an `out`:

- it fires the `rise` action when the **applied** value goes from false to true, and `fall` when it goes from true to false;
- **at the start of each generation**, it executes once the action for the `safe` level (synchronization), so that the equipment is left in a known state;
- the keepalive (document 01, §8) does **not** apply to members with `edge`, and `jk check` requires the equipment to have its own communication fallback (`J0205` in `prod`);
- the supervisor's `force_safe` bit (document 01, §3.4) is honored by the driver, which executes the `fall` action directly, without going through the group.

### 5.3 Composite and virtual instances

```toml
[instances.p1]
class = "std/pump@1"
capabilities = ["pressure"]
map = { run_cmd = "io.do_1", running = "io.di_1", fault = "io.di_2", pressure_out = "ai.ch1" }

[instances.p2]
class = "std/pump@1"
implemented_by = { app = "pump_ctl", bind = { contactor = "io.do_3", feedback = "io.di_3", thermal_trip = "io.di_4" } }

[instances.p2.actions.reset]
role = "operator"
```

- `implemented_by = "app:<name>"` uses the `bind` of `[apps.<name>]` and serves **one** instance. The table form (`{ app, bind }`) creates **one app instance per virtual instance**, each with its own bindings. This is how three pumps share the same implementation.
- The implementation receives the `out` as commands (`io.cmd.*`), publishes the `in` (`io.state.*`), emits the `event` and handles the `action` with the `on-action` export (ref/04 §2.6 and §4.8.4).
- In a composite instance, the value of a class member in the `map` is the name of an instance (or a list, in a collection); in a virtual instance, the `map` may exist alongside `implemented_by`, only with the class members (ref/01 §5.13, §8).

---

## 6. What `jkc` generates

```
classes/std/pump.jkc
  │ jkc check    parse → packages → types and units → rules (§7)
  ▼
pump@1.0.0.contract.json    canonical form, sorted, no comments
  ├─ contract hash      (members, types, methods, queries, capabilities) → joker.lock
  └─ documentation hash (docs, HMI annotations)                         → does not break compatibility
  ├─▶ Rust    crate joker-classes-std   CycleIo/TaskIo handles, enums, records, methods (emit records)
  ├─▶ TS      @joker/classes-std
  ├─▶ Python  joker_classes.std
  ├─▶ docs    page for the registry and for the HMI
  └─▶ OPC UA  ObjectType + Methods for the bridge (M11)
```

The `.contract.json` files go to the device inside each generation (with the manifest and the lock), and the core's instance registry is built from them.

---

## 7. `jkc check` rules

| Code | Rule |
|---|---|
| `JC0107` | physical quantity without a unit |
| `JC0201` | method assigns to an `out` more than once |
| `JC0204` | method assigns to something that is not `out` |
| `JC0205` | `atomic` without `order`, or `order` with unassigned members |
| `JC0206` | argument with the same name as a member |
| `JC0207` | reserved name (`name`, `index`, `health`, `read`, `raw`, `has`) on a member, query, method, event or action |
| `JC0210` | use of a member of a capability outside it |
| `JC0301` | expression with incompatible dimensions |
| `JC0310` | queries in a cycle |
| `JC0320` | query with a window above 15 min |
| `JC0401` | action used in a method (including a member's: `compressor.reset()`) |
| `JC0402` | action without `@timeout` |
| `JC0501` | composition with depth > 3 (counted as in §8) |
| `JC0502` | method that calls a method of a collection member, or that iterates over it |
| `JC0503` | call to a method that the member does not have, or of a capability that the member does not require with `@requires` |
| `JC0504` | `@requires` with a capability that the member's class does not have |
| `JC0601` | version incompatible with the previous one without bumping the major (§9) |
| `JW0230` | method conditioned on an `in` (looks like an interlock; check that the manifest also has it) |

**Two limits of 3, which do not add up.** `JC0501` belongs to `jkc` and counts the **class composition** (§8). `J0620` belongs to `jk check` and counts the **implementation layers**: virtual instances whose app uses other virtual instances (document 14, §7.2). A `heat_pump@1` with depth 3, implemented by an app that uses a virtual compressor, has depth 3 and 2 layers, and passes both.

---

## 8. Composition and collections

```
class station@1.0.0 {
    member pumps: std/pump@1[1..8]
    member tank:  local/tank@1          // std/ has no tanks (14 §10)
    in flow: f64 m3/h

    query any_pump_failed: bool = any(pumps.failed_to_start)
    query pumps_running:   u64  = count(pumps.running)
}
```

The aggregations (`any`, `all`, `count`, `sum`, `maxs`, `mins`) are the only way to "iterate" in a class. A method of a composite class does not iterate (`stop_all()` is not expressible, `JC0502`): behavior over collections goes into an app or into an active implementation. Over a fixed member, a method may call its methods (§4.1).

**Depth.** It is counted along the longest chain of class members starting from the class, counting the class itself as 1: `station` = 1, the `pump` of `pumps` = 2, a `drive` inside the pump = 3. A class with a chain of 4 is `JC0501`; a class that contains itself, directly or indirectly, is too, because the chain does not end. Implementation layers are counted separately (§7, `J0620`).

**The rest of the class member rules are where the rule lives:**

| Subject | Rule | Where |
|---|---|---|
| binding a member in a composite instance | the value in the `map` is the name of an instance; in a collection, a list of names | ref/01 §5.13 (`J0650`–`J0654`) |
| paths | `<instance>.<member>.<submember>`; in a collection, with the name of the element's instance (`station.pumps.p1.running`) | document 16, P6 |
| one instance in several compositions | allowed; a single envelope and a single arbiter per `out` | document 16, P6 (`J0655`) |
| methods that call members' methods | fixed members only | §4.1 |
| health | the worst between the class rule and that of the members | §4.5 |
| a profile that fulfills a composite class | `members.<member>` subtables | ref/03 §2.2.14 |
| generated handles | `io.esp.boot.clicked()`, `io.station.pumps.iter()` | ref/04 §4.8.6, ref/05 §14.6 |
| on a Joker Micro | the catalog carries the tree | ref/07 §6.3 |

---

## 9. Versions

| Change | Classification |
|---|---|
| adding a capability, `@optional` member, query, method, event, action | minor |
| documentation or HMI annotations | patch (does not change the contract hash) |
| unit to another of the same dimension (`W` → `kW`) | minor (the runtime converts) |
| dimension, type, direction (`in` ↔ `out`), deleting or renaming a member | major |
| renaming with `@renamed_from("old")` | minor, with an alias for one major |
| making an optional member required | major |
| changing the body of a method or of a query | major |
| adding a field **at the end** of a `record` or of an `event` | minor |
| reordering, removing or inserting in the middle a field of a record | major |

Records (events, arguments and results of actions and methods) travel in CBOR as a map with **integer keys = field position** in the contract (ref/00 §4.1, D2 of document 22). That is why, within a major, fields are only added at the end: an old reader ignores the keys it does not know, and a new field must not take the position of another.

`jk class diff <a> <b>` applies this table and is run by `jk class publish`.

---

## 10. Why not WIT, OPC UA NodeSet, Protobuf or IEC 61131

| | `.jkc` | WIT | OPC UA NodeSet | Protobuf/gRPC | IEC 61131 FB |
|---|---|---|---|---|---|
| checked units and quantities | yes | no | declared, not checked | no | no |
| desired state with lease and `safe` | yes (`out`) | no | no | no | partial |
| request/response | yes (`action`) | yes | yes | yes | no |
| events | yes | yes | yes | streams | no |
| derived values evaluated by the system | yes (`query`) | no | no | no | in code |
| optional capabilities | yes | no | partial | no | no |
| analyzable without executing | yes | yes | yes | yes | no |

None combines units, safe desired state, request/response and derived values. The OPC UA bridge generates an ObjectType from each class, and so `.jkc` does not compete with OPC UA: it feeds it.

---

## 11. Stress cases that defined the language

The 15 cases of the original proposal (pulse start, reset, battery with mode and dynamic limits, inverter manufacturers, EV charger, firmware, EEPROM, double `toggle`, SCADA calling methods, `stop_all`, conditional methods, reading an `out`, mixed versions, language growth) are in `proposals/class-idl.md`, §10. The fixes the readiness round made to them are in document 18, §2.

**Reading an `out`:** it gives the **applied** value. `x.mine()` gives the writer's own proposal, `x.winner()` the level that won. In the class's expressions, `x` is the applied value and `own(x)` is the caller's.
