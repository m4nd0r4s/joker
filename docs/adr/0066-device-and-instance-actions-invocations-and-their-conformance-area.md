# ADR 0066 — Device and instance actions: the invocation lifecycle, and the `action` conformance area

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0058 placed the step interpreter in `joker-hal` and added the `action` decision record; ADR 0060 made permits fail closed and step conditions honour quality. ref/09 §5.10, ref/06 §2.10–§2.14, ref/03 §2.2.12 and ref/04 §7.1 specify the rest of an invocation: the order (role, permissive, deduplication, the device's serialized queue, the program, the result), the body (`args`, `timeout`, `reason`, `wait`), long actions with progress and cancellation, and the error codes. A review of `main` found the runtime short of that text, and some of what it needs is not decided anywhere:

1. **The queue.** ref/09 §5.10 and document 17 §4.2 put a "serialized queue of the device" after deduplication, and give `J-E1403` for a full queue, but say neither how deep it is nor what "busy" means. The runtime refused a second invocation while one was running.
2. **The deduplication cache.** document 16 E9 fixes the window (60 s by default), but not how many keys the node keeps, what happens when the same key arrives while the first invocation is still running, or what a key reused with other arguments is compared against.
3. **Invocations.** ref/09 §3.13 makes an invocation a resource (`/v1/invocations/{id}`), but does not say how long a node keeps one, whether it survives a restart, or how a short action that outlives `wait` is reported.
4. **The `action` record.** ADR 0058 §7 asks for `duplicate_of` as "the `at_mono_ns` of the earlier completion" and for `tick`; neither says which tick (an action belongs to no cycle group) nor how an audit joins a replay to its original when the original has not completed yet.
5. **Deadlines.** ref/06 §2.14 gives `invoke` the action's `timeout` and `read-addr`/`write-addr` the device's `timeout` per step; nothing says what bounds a step program as a whole.
6. **Results and failures of a program.** ADR 0058 §6 and ADR 0060 §5 name the failures; ref/09 §5.10 and ref/04 §7.1 name the API kinds and codes. The projection of a `fail_if`, an unresolved step, a quality failure and a cancellation onto those codes is not written down, nor how an `Uncertain` value of a `read` step appears in the result.
7. **Instance actions.** document 17 §4.2 routes an instance action to "the profile steps (or the virtual implementation's `on-action` task)", and ref/01 §5.13 maps a class `action` member to `"<device>.<action>"`; the runtime has no instance routing at all.
8. **Conformance.** ref/03 §2.6.1 knows three suite kinds: `core` (no I/O), `class` and `driver`. An action is the runtime's dispatch above a driver: it is I/O, so `core` cannot express it, and it is not a property of one driver either, so `driver` does not fit. The cases lived in a private `jokerd` test that `cargo xtask conformance` did not run, with a double that never executed the steps.

## Decision

### The invocation lifecycle

1. **Order.** An invocation is checked in the order of ref/09 §5.10: the role (`J-E1002`), the arguments against the action's `args` (`J-E1401`) and the requested `timeout`, the permissive (`J-E1402`), then deduplication, then the device's queue. A refusal at any of these steps is a request error (no invocation runs) and enters the decision log as an `action` record with outcome `rejected_forbidden`, `rejected_invalid`, `rejected_not_permitted`, `rejected_conflict` or `rejected_busy`.
2. **The device queue** holds at most **8** admitted invocations per device (running and waiting together), served one at a time in arrival order by the device's io task. A ninth is `rejected_busy` (`J-E1403`, 503). "Busy" means "queue full", never "one action is running".
3. **The deduplication cache** keeps one entry per `(target, action, key)`, holding the invocation id, the canonical hash of the request body (`args`, `timeout`) and the instant its window starts. An entry stays for the action's `dedup_window` after its invocation finishes, and for as long as the invocation is running. The node keeps at most **4096** entries; expired entries are dropped before each admission, and when 4096 live entries remain the new invocation is `rejected_busy` (`J-E1403`): a key the node promised to remember is never forgotten early.
   - The same key within the window, with the same body, returns the original invocation with `replay: true` (`Joker-Idempotent-Replay: true`). If the original is still running, the replay waits for it like the original caller would (rule 6).
   - The same key with another body is `rejected_conflict` (`J-E1024`, 422).
   - A replay is not checked against the queue, but it is checked against the role, the arguments and the permissive first (rule 1).
4. **Timeout.** The effective timeout of an invocation is the manifest's (`[devices.<d>.actions.<a>].timeout` or `[instances.<i>.actions.<a>].timeout`), or else the profile's (ref/03 §2.2.12), shortened by the request's `timeout` when that is shorter. A request `timeout` longer than the configured one is `rejected_invalid` (`J-E1401`). The clock of the timeout starts when the device's io task starts the program, not while it waits in the queue. It bounds the whole program: a `wait`, an `until` or a driver call still running at the deadline is dropped, and the invocation fails with `timeout`. Every `read-addr`/`write-addr` of a step is further bounded by the device's own `timeout` (ref/06 §2.14); a step that hits that bound fails with the driver's own `timeout` failure, projected as `device`.
5. **Arguments.** `args` is validated against the action's `args` table (ref/03 §2.2.12): every declared argument is required, no undeclared one is accepted, and each value must fit its type (`bool`; `u64` a non-negative integer; `i64` an integer; `f64 <unit>` a number or a quantity literal in a unit of the same dimension, converted to the declared unit; an enum of the profile, a variant name or its integer value, ref/03 §2.2.8). A `"$name"` in a step is replaced by the converted value before the program reaches the interpreter; a `$name` the action does not declare is `J0821` when the profile is loaded.
6. **Waiting.** A short action with `wait: true` (the default) answers when it finishes, or after 30 s (the longest `wait_timeout` of ref/09 §3.13) with 202 and the invocation still `queued` or `running`. A long action (`long = true`) and any action invoked with `wait: false` answer 202 at once with the invocation `queued`. The invocation is then followed at `/v1/invocations/{id}`.
7. **Invocations are volatile.** The node keeps the last **1024** invocations in memory (a running or queued one is never evicted); they do not survive a restart of `jokerd` nor a generation swap that stops the device. An invocation interrupted by either ends `failed` with `kind = device`, `J-E1415`. The decision log is the durable record (ADR 0058 §9).
8. **States.** `queued` (admitted, waiting in the device's queue), `running`, `succeeded`, `failed`, `cancelling`, `cancelled`. A timed-out action is `failed` with `error.kind = timeout` (ref/09 §5.10); `timed_out` is not used. `started_at` and `finished_at` are set when the io task starts and ends the program.
9. **Cancellation.** `POST /v1/invocations/{id}/cancel` on a long action that is `queued` or `running` sets it `cancelling`; on anything else it is `J-E1405` (409). A queued one is cancelled before it runs. A running step program checks the request before each step; a running native action sees it through `Cx::cancelled` (ref/06 §2.10). Either ends `cancelled`, with `error.kind = cancelled`, `J-E1409`. A cancellation is the principal's own decision and is recorded with the invocation's `action` record.
10. **Progress.** A long step program reports `fraction` = completed steps / steps after each step. A long native action reports what its driver last passed to `Cx::progress`, read when the call returns.
11. **Holding no lock across the driver.** The controller resolves the target, evaluates the permissive and admits the invocation under its own lock, then releases it; the program runs on the device's io task and its completion is recorded by the invocation registry, which has its own lock. No lock of the controller is held while a driver runs.

### Results and failures

12. A program's `read` steps fill the result record by `into`. A `Good` value is the field's value; an `Uncertain` one is `{ "value": <value>, "quality": "<literal>" }` with the quality literal of ref/03 §2.1.3, so its flag never disappears (invariant 4). A native action's result is the CBOR record the driver returns, as JSON.
13. The projection onto the API (ref/09 §5.10) and onto an app's `action-error` (ref/04 §7.1) is:

| Failure | `error.kind` | `code` |
|---|---|---|
| the action's timeout, an `until` that never matched, `ActionFailure::Timeout` | `timeout` | `J-E1406` |
| `read-addr`/`write-addr` failed, `ActionFailure::Device`, a `Bad` or untrusted `Uncertain` read (ADR 0060 §5) | `device` | `J-E1407` |
| a `fail_if` that matched (its message), `ActionFailure::Invalid` | `invalid` | `J-E1408` |
| a step the interpreter cannot resolve (ADR 0060 §7), `ActionFailure::Unsupported`, the driver task gone | `device` | `J-E1408` |
| the device not connected (its link is down or its driver is not running), `ActionFailure::NotConnected` | `device` | `J-E1415` |
| cancelled (rule 9), `ActionFailure::Cancelled` | `cancelled` | `J-E1409` |

### The `action` record

14. ADR 0058 §7's record gains: `invocation` (the id), `target` (the name invoked: the device, or the instance), `instance` (only for an instance action), `args` and `reason` (as received), `result` (on `ok`), `code` and `kind` (on `failed`, the projection of rule 13), and `failure` (the detail). `tick` is the last tick of the first cycle group of the running generation when the invocation was admitted. `at_mono_ns` is the admission instant.
15. `duplicate_of` is the `invocation` id of the original, and `duplicate_at_mono_ns` its `at_mono_ns`; the original's own record carries the same `invocation`, so an audit joins the two whether the original had completed or not. A replay's record is written when it is answered, with the original's outcome, or `outcome = "in_progress"` when the original has not ended by then.
16. A record the decision log cannot take is never ignored: a request whose answer depends on it (a refusal, a replay, a waited invocation) fails with `J-E1000` (500) naming the log error, and an invocation that completes with nobody waiting reports the error on the daemon's error output.

### Instance actions

17. An **implicit** instance (a device whose profile declares `[implements]`) has the class action `m` when the profile maps `m = "actions.<a>"`; the instance shares the device's name (ref/03 §2.2.14), and the invocation runs the device's action `<a>` with the device's configuration.
18. A **composite** instance (`[instances.<i>] map`) has the class action `m` when the map binds `m = "<device>.<action>"` to an action of that device. The invocation is configured by `[instances.<i>.actions.<m>]` (`permit`, `role`, `timeout`, `dedup_window`) and otherwise by the device action's own; it is queued on the device's queue and runs the device's program. Its record names the instance in `target` and `instance`, and the device in `device`.
19. A **virtual** instance (`implemented_by`) is served by the app's `on-action` (ref/04 §4.3.3), which the runtime does not host yet: `GET /v1/actions` lists none of its actions, and invoking one is `J-E1400`.

### The `action` conformance area

20. A fourth suite kind, `action`, is added to ref/03 §2.6.1. Its files are `conformance/actions/<suite>.conformance.toml` (`J0845` elsewhere), run by `cargo xtask conformance` on the `linux` host only (`hosts = ["linux"]`; the MCU runner skips them). The cases are written from the specification only, like every other area.
21. **Fixture.** `[fixture] manifest` is a manifest in ref/01 syntax, verbatim; `[fixture.profiles]` maps each profile reference the manifest uses (`"acme/pcs@1"`) to the profile's text (ref/03 §2.2). The runner activates the manifest as a generation, so the plan, the permits and the decision log are the node's own. Every device of the manifest runs its real io task with a **register double** in place of its driver: `[[double]]` with `device`, `registers` (the initial value of each address, keyed by the address's `at`), and optionally `on_write` (a list of `{ at, value, set = { <at> = <value> } }`: writing `value` at `at` also sets those registers), `quality` (`<at> = "<quality literal>"`: the double reports that quality for reads of that address), `fail` (`<at> = "device_fault" | "timeout" | "comm_failure"`: reads and writes of that address fail with that `batch-failure`), `link = "down"` (every `connect` fails), and `[double.native.<action>]` with `result` (a table, the native result), `failure` (`timeout`, `device`, `invalid`) or `hold` (a duration the call runs before it answers, checking for cancellation). The double serves `read` (for the device's points, by their `at`), `read-addr`, `write-addr` and `invoke`; it records every write in order. So the steps of a program run for real, through the interpreter, against it.
22. **Timeline.** Each case starts from a fresh node with the fixture's generation. `do` holds the actions, executed in order; `at` is the earliest instant, counted from the start of the case in real time, at which the runner starts the action:
    - `{ at, invoke = "<target>.<action>", key, ref, args, timeout, reason, wait, role }`: an invocation by a principal with that `role` (default `operator`); `ref` names it for the expectations (default: the key);
    - `{ at, cancel = "<ref>" }`: cancels that invocation;
    - `{ at, settle = true }`: waits until every invocation is finished.
23. **Expectations** (checked after every action of the case ran and every invocation finished):
    - `{ invocation = "<ref>", state, result, error, code, message, replay, rejected, progress }`: the invocation as `/v1/invocations/{id}` shows it (`error` is `error.kind`, `message` a part of `error.message`), or `rejected = "<code>"` for a request error;
    - `{ decision = "<ref>", outcome, kind, code, duplicate, count, instance, device }`: its `action` record (`duplicate = true` requires `duplicate_of` to name the original);
    - `{ writes = "<device>", equals = [[at, value], …] }`: every write the double received, in order;
    - `{ register = "<device>", at, equals }`: a register at the end of the case.
24. A malformed suite is `J0887`; a failed expectation is `J0842`.
25. The runner is `cargo xtask conformance`, which therefore depends on `jokerd`. This amends ADR 0057 §2: `xtask`, the repository's task binary, may depend on `jokerd` like `joker-cli` does; nothing depends on `xtask`, so the reason for the rule (no layer below the daemon reaches it) still holds. `policies/dependencies.md` and the graph test of `joker-supply-chain` list both. The `action` cases run on the `linux` host only: the QEMU runner skips their files, and the determinism comparison between hosts leaves the `actions.*` suites out.

## Consequences

- ref/09 §5.10 states rules 2–3, 6–9 and 19; ref/03 §2.6.1 lists the `action` kind and §2.2.12 the argument types of rule 5; conformance/README.md documents the area.
- `jokerd::actions` owns the invocation registry, the deduplication cache and the device queue; `jokerd::drivers` runs step and native programs with the deadlines of rule 4, the cancellation of rule 9 and the projection of rule 13; `joker-hal`'s interpreter resolves `"$name"`, honours cancellation and reads with the step's own type.
- The API gains `GET /v1/invocations`, `GET /v1/invocations/{id}` and `POST /v1/invocations/{id}/cancel` exactly as the OpenAPI already specifies them; no contract changes.
- The caller's role comes from the API's one resolver of ADR 0063, which now also reads the running generation's `[security.roles]` (ref/01 §5.20): `local:user` and `local:group` rules, `operator` and `admin` defaulting to the groups `joker-ops` and `joker-admin` when the manifest does not declare them, anyone `admin` in `dev` without `[security.roles]`, and uid 0, the `joker` user and the account of `jokerd` always `admin`. This settles ADR 0063's consequence that the generation's roles were not applied yet; `/v1/info`, the WebSocket and the actions endpoints all use it.
- An action's `permit` is compiled by the single expression path of ADR 0068 (`expr_plan::bind`); a composite instance's permit is sized by the period of its device's groups.
