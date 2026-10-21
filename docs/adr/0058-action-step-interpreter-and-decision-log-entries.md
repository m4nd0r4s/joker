# ADR 0058 — The step interpreter lives in `joker-hal`; a new decision-log record kind for actions

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/06 §2.10–§2.14 already settles almost everything about how an action actually runs: the driver only offers `read-addr`/`write-addr` (capability `raw-access`) or `invoke` (capability `native-actions`); the step language of ref/03 §2.2.12 is interpreted by "the core", not by the driver; the permissive, the role, the per-device serialization and the idempotency key belong to the core as well; `action-failure` has a fixed taxonomy that the core projects onto the apps' `action-error` (ref/04). AUR-405 already built the manifest-side validation (`permit` type-checked, `J0304`; the action checked against the profile's declaration, `J0101`/`J0142`) and the dispatch decision itself (`jokerd::actions::Dispatcher`: serialize per device, dedup within `dedup_window`, gate on `permit` — commit 92ced65). None of that executes anything yet.

Two things are not decided anywhere:

1. **Where the step interpreter lives.** "The core" in ref/06 is the runtime that calls drivers, not `joker-core` specifically (invariant 1: `joker-core` does no I/O). The interpreter needs `Cx` (for `sleep_until`/`within`/deadlines) and the driver's `read_addr`/`write_addr`/`invoke`, all of which are `joker-hal` host-side concepts. It also serves more than user-invoked actions: ref/06 §2.10 says the same steps run `[connect]`, `[relinquish]`, `[config] apply` and the fallback (M1) — every driver gets those for free through the same engine, not through per-driver code (the `sim` driver is the stated exception: its `[actions]` are native, ADR 0054).
2. **The decision log has no record kind for actions.** ADR 0033 fixes exactly two kinds, both tied to an output (`output` and `write` records, keyed by output path and applied value); an action is not an output and does not fit either shape. `jk decisions` and any future `jk why` need *something* to show for an invoked action, and today there is nothing.

## Decision

### Where the interpreter lives

1. The step interpreter is a new `joker-hal` module (host-side, `alloc` feature), shared by every tier-N driver through `read_addr`/`write_addr`/`invoke`, the same way `commit` (ADR 0047) and `acquire` are shared today. It is not part of `joker-core` (it does I/O) and not duplicated per driver crate.
2. A compiled step program is a flat, borrowed sequence of steps (`write`, `read`, `wait`, `until`, `fail_if`, ref/03 §2.2.12), built once when the profile is loaded (where `[actions.<a>].steps`'s static checks already belong per ref/03 §2.2.12: a bad address, an unknown step, a `wait` over the timeout or an undeclared `$arg` is `J0821` at that point, not at invocation time). The interpreter itself only walks an already-validated program; it does not re-validate.
3. Execution is driven by the same `Cx` a driver already receives, so `wait`/`until`'s timeout and the action's own `timeout` (ref/01 §5.8.2) compose with `Cx::within`, exactly as `exchange` does in `modbus`'s driver today.
4. One connect, relinquish, config-apply or fallback step program runs the same way a user-invoked action does, through this one engine; the only thing that differs between them is who decided to run it and whether the invocation goes through `Dispatcher` (user actions do; the lifecycle hooks do not, since they are not arbitrated invocations with a permissive, a role or an idempotency key).
5. A native action (`[actions.<a>] native = { … }`, capability `native-actions`) skips the interpreter entirely and calls `Driver::invoke` directly; this path is already fully specified in ref/06 §2.10 and needs no new decision.

### Error projection

6. The interpreter's own failures (`fail_if` triggered, `until` timed out, a step's `BatchFailure` from `read-addr`/`write-addr`) are projected onto `ActionFailure` using the existing mapping of ref/06 §2.10: a transport-level `BatchFailure` becomes `ActionFailure::Device`/`NotConnected`/`Timeout` as appropriate; `fail_if`'s declared message becomes `ActionFailure::Invalid`. No new failure taxonomy is introduced.

### Decision log

7. A third decision-record kind, `action`, is added alongside ADR 0033's `output` and `write`. It is appended through the already-generic `DecisionLog::append` (no change to that API) with:
   - `record`: `"action"`;
   - `device`, `action`: the names;
   - `key`: the idempotency key;
   - `tick`, `at_mono_ns`: when the dispatcher admitted it (or served it from the dedup cache);
   - `writer`: who invoked it, in the same shape `writer_json` already produces for writes (origin, level where applicable);
   - `outcome`: `"ok"` (with the result record), `"rejected_busy"`, `"rejected_not_permitted"`, or `"failed"` (with the projected `ActionFailure` and, for `fail_if`, its message);
   - `duplicate_of`: the `at_mono_ns` of the earlier completion, present only when `Dispatcher::begin` returned `Duplicate` — so an audit never mistakes a cached fast-path for a second real invocation.
8. Every invocation enters the log exactly once, admitted or not (the same invariant 8 that already covers writes): `Dispatcher::begin` returning `Rejected` is as much a decision as `Admit` is.
9. Durability follows ADR 0033 §11's classification by default: an action record is **immediate** (fsync), on the same reasoning as trips and manual writes — an invoked action is a deliberate, infrequent, audit-relevant event, never a per-tick one.

## Consequences

- `jokerd::actions::Dispatcher` (commit 92ced65) is unchanged: it still only decides whether to run something. This ADR fixes what runs when it says `Admit`, and what gets logged either way.
- A new `joker-hal` module needs its own `no_std`+`alloc` build check, same as `commit`/`acquire`.
- `jk decisions` and any future `jk why` read a `record` field that now has three values instead of two; both already treat the decision log as a sequence of generic JSON objects, so this is additive, not a breaking change to the file format.
- Implementing the interpreter still needs, as a prerequisite, a real profile loader that produces a compiled step program from `[actions.<a>].steps` — today profiles are a hand-written catalogue in `jokerd::catalog` (M0), with no action ever declared on the bench profile. That loader is tracked separately (it is also what full manifest sections, `F0-MANIFESTO-COMPLETO`/AUR-226, already need for other profile keys).
