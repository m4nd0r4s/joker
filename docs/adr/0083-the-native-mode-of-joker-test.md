# ADR 0083 — The native mode of `joker-test`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Task 19 of document 13 asks for `joker-test`, the harness with a virtual clock, "still without Wasm: native test tasks", and AUR-241 asks for the tests of the track app to run on it. ref/04 §6 describes the whole harness, in two modes (§6.1): `jk test` runs the real `.wasm` component in wasmtime, and `cargo test` links the app into the test binary. Three things were not written down:

- how the native mode runs an app the same way the host does, without the component around it;
- how it learns what the component carries in its `joker:meta` (the periods of the tasks, the defaults of the parameters), which on a native target is not emitted (the custom section only exists in Wasm);
- how a test asserts the level that won an output, which task 19 asks for ("assertions on outcomes, levels and trace") and §6.4 does not offer.

## Decision

1. **Same exports, same bytes.** The native mode calls the `init` and `on-cycle` exports of the SDK (`joker::__private::Runtime`) in process, with the `init-context` the host builds, the packed image of ref/04 §3.2 and the proposal buffer of §3.3, written and read with `joker_host::abi`. The proposals are staged into a `joker-core` process image and committed by the real arbiter and envelope, at a virtual instant: tick `k` is at `k × period`.
2. **The metadata comes from the SDK's registrations.** `#[joker::app]` and `#[joker::cycle]` already register the app and its tasks with `inventory` on every target; the registrations now also carry their `joker:meta` fragment, the same bytes the component carries in its section. `joker_host::meta::from_fragments` merges them with the rules of the host. The harness reads the periods, the parameter defaults, the port types and units from there.
3. **One harness at a time.** The SDK keeps the state of the app in one static, as a component does. A `Harness` holds a process-wide lock for its lifetime, so tests in one binary that build one run one after the other.
4. **The cycle subset first.** The native mode serves what the SDK serves today: inputs, outputs and parameters of scalar types, cycle tasks of one group, constant `range` and `rate`, the writer's level and lease (default 3 × the period, docs/16 A9), errors returned by the tasks, and the traces of the outputs. The builder methods, the execution and the assertions follow §6.2–§6.5 with their names. The rest of §6 (the Wasm mode, devices, events, actions, history, the scenario) comes with the SDK features it tests; `joker-spec-tests` lists what is missing (ADR 0080).
5. **Levels.** `OutputTrace::levels()` gives, for every tick, the level that won the output, or `None` for the safe state. It is added to ref/04 §6.4.
6. **Warning.** The first harness of a process prints that it runs the app natively and not as the production component, as §6.1 says.

## Consequences

- `joker::__private::AppEntry` and `CycleTask` gain a `meta` field (hidden API, filled by the macros); `joker-host` gains `meta::from_fragments`.
- `crates/joker-test` exists, and the track app of `examples/bench/apps/light` runs the two tests of track part 1 §9 unchanged, with `cargo test`.
- A test that uses the prelude has `QualityReason::*` in scope, as §6.5 says, and so its `None` is `QualityReason::None`, not `Option::None`. AUR-433 tracks the rest of §6 and this point of the reference.
