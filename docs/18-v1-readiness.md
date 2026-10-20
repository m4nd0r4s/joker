# 18 — Readiness round: are we ready to close v1 and implement?

The last stress round before freezing the documentation. Three fronts:

1. a **full audit** of the 56 files, looking for contradictions, specification gaps and errors (45 findings);
2. **nine end-to-end attacks**, each following one feature through every layer, already with the `.jkc`;
3. a **name check** (crates, CLI, vocabulary).

Conclusion in §6: **yes, v1 may be closed**, with four decisions that are not technical and that belong to the project owner.

---

## 1. End-to-end attacks

Each attack follows a request from the code of whoever writes it to the equipment and back, and asks at each boundary: *who owns this step, and is it specified?*

### P1 — A method, from the `.jkc` to the Modbus register and back

`classes/std/pump.jkc` → `jkc` generates the Rust handle → the app calls `p1.run_at(60 %)` → record in the proposal buffer → expansion in the core → arbiter → envelope → driver box → FC16 → readback → trace → replay.

| Boundary | Finding | Fix |
|---|---|---|
| app → core | the proposal buffer only had 12 B writes, with a `u16` port; there was no way to carry a method call, and `u16` limited an app to 65,535 ports | typed records: write (14 B, `u32` port) and method (10 B + arguments) — document 04, §2 |
| core | method expansion had no step in the tick, and `own()` did not exist in the expression language | step 5a (document 01, §4); `own()` in document 02, §10 and in 16, §4 |
| trace | method calls were not recorded, and replay could not reproduce them | method records in the trace format (document 16, §7) |
| `jk check` | the app metadata had neither equipment nor classes | `joker:meta` with equipment, classes and contract hashes (document 04, §2) |

### P2 — An action on a virtual instance

`jk action p2.reset --key k1` → permissive → role → deduplication → … and then? The implementation is a Wasm app, and the WIT world had no way to deliver the action to it. **Fix:** export `on-action` (document 04, §2). And where is the permissive of an action of an instance that is not a device configured? **Fix:** `[instances.<i>.actions.<a>]`, with precedence over the class suggestions (document 17, §4.2).

### P3 — The pulse-start pump (`edge` adapter)

Three dangerous interactions that nobody had seen:

- the **keepalive** (document 01, §8) would resend the start pulse every 5 s;
- the supervisor's **`force_safe`** bit writes to a value box, but an output with `edge` needs a stop **action**;
- at the **startup** of a generation, the equipment may be running without Joker knowing it.

**Fix** (document 17, §5.2): no keepalive on `edge` members, and a communication fallback required in the equipment itself; `force_safe` executed by the driver as the `fall` action; and synchronization at startup, executing the action of the `safe` level.

### P4 — Three pumps, one implementation

Document 14 bound `p1`, `p2` and `p3` to the same `pump_ctl`, which had **one** `bind`. Three pumps with one contactor. **Fix:** `implemented_by = { app, bind }` creates one instance of the app per virtual instance (document 17, §5.3; document 03, §9.2).

### P5 — Layers with zero latency

Document 14 promised zero latency between the business app and the implementation in the same group, but in the tick of document 01 the arbiter only ran after all tasks, and the `order` key did not exist. **Fix:** step 4b (virtual commands arbitrated right after the task that produces them) and `order` in cycle groups (document 03, §7).

### P6 — A new generation in the middle of a ramp

A configuration is applied while the PCS rises from 20 to 80 kW at 25 kW/s. What is the "last applied value" of the new generation? And does the ramp continue? The specification did not say. **Fix:** document 16, A7 (the effective value goes through the envelope on every tick) and A8 (a generation that keeps the output inherits the last applied value).

### P7 — The track as the acceptance test of Phase 1

Exit gate 4 of Phase 1 is "do the track with the documentation only". When walking it against the roadmap (the old track, of eight parts; today's, of eleven parts on the bench of document 23, keeps the gate at parts 1–7, document 11):

- part 8 uses virtual instances, which belong to Phase 2 → the gate becomes **parts 1–7**;
- the Phase 1 `std/` library had neither `humidity_sensor` nor `pump` → added;
- the dynamics of the simulated plant **failed its own unit check** (`%` subtracted from `m3/h`) → fixed;
- the I²C used a file path where the manifest expects a bus → `[buses.i2c1]`;
- part 4 promised a `Substituted` quality on outputs that the runtime cannot produce → removed, with the rule in document 16, §3.1.

### P8 — The names

- **`--device` versus `[devices]`.** In the CLI, `--device` chose the board with `jokerd`; in the manifest, `[devices]` are the equipment it talks to. In an installation, `jk read sala.temp --device sala` would be ambiguous to anyone. **Fix:** the concept is now called **node** (`--node`, `jk node enroll`, `JK_NODE`), and it enters the vocabulary of document 15.
- **crates.io** (checked on 2026-09-24): the name `joker` already exists (a JavaScript tokenizer from 2015) and so does `jk`. `joker-core`, `joker-sdk`, `joker-host`, `joker-runtime`, `jokerd` and `jkc` are free. **Decision:** all published crates use the `joker-` prefix; the SDK is the `joker-sdk` crate, imported in apps as `joker` (`joker = { package = "joker-sdk", … }` in the `Cargo.toml` generated by `jk app new`), and the CLI is the `joker-cli` crate with the `jk` binary.
- **Trademark.** "Joker" is a widely used name. A trademark check is legal work and was not done. It remains a decision of the owner (§6).

### P9 — Can Phase 0 start with the documentation only?

The audit found nine areas that blocked concrete Phase 0 tasks: trace format, schema of the conformance cases, ~12 rules of the arbiter and the envelope, quality table, edge cases of expressions, unit catalog, manifest schema, code registry and alias semantics. **Fix:** document 16 (core semantics) closes them and now takes precedence over the other documents in those matters. The JSON schema of the manifest is now **generated** from the types of `joker-manifest`, with document 03 as the source.

---

## 2. Audit fixes

The 45 findings of the audit, grouped. All are fixed in this version, or explicitly reclassified in document 16, §10.

| Group | Findings | Where it landed |
|---|---|---|
| method execution, WIT, metadata | method in the proposal buffer; `own()`; `joker:meta` with classes; `on-action`; `u32` port | 01 §4, 02 §10, 04 §2, 16 §4, 17 §4 |
| virtual instances and layers | one app instance per virtual instance; step 4b; `order` | 03 §7 and §9.2, 14 §7 and §9, 17 §5.3 |
| defaults and codes | `schedule_depth` = 0; `J0352` separated from `J0351`; single code registry | 02 §7.2, 03 §15, 16 §9, old track 01 |
| lock | `[classes]`, `[wcet]`, `accept_missing_fallback` | 03 §14 |
| instances | `sim = true \| table`; `const:` forbidden in `out`; one envelope per signal; path collisions | 03 §9.2, 14 §5.2, 16 §6, 17 §5.2 |
| quality | normative table of `get`/`get_uncertain`/`get_or`; no automatic contamination of outputs | 16 §3, 02 §4, old track 04 |
| CLI | `jk call`, `jk action` on instances, `jk runtime build`, `jk migrate`, `jk read --refresh`, `--from/--to`, `jk class fmt/check/diff/doc/gen`, `--node` | 07 |
| versions | examples aligned with `joker:runtime@0.1.0`; the manifest's `joker` as the minimum version of the runtime and of the schema | 03, 04, 09, track |
| track and roadmap | DX gate = parts 1–7; `std/` with `humidity_sensor` and `pump` | 11, track README |
| paths and names | instance aliases; `to` of pipelines; apps in `snake_case`; `[buses.i2c1]`; channel `a0` | 02 §9, 04, 16 §6, old track 04, example 00 |
| model KPIs | own grammar, separate from the control expressions | 06 §10.1 |
| `.jkc` | full version in the declaration; `map` with adapters; `std/` with slash and implicit; method response in async; return types and lists; precedence of actions; `edge` vs keepalive and `force_safe`; health rule; pump with `variable_speed` | 17 |
| figures and facts | target RSS 60 MB (and Phase 1 gate at 90 MB); `pump_ctl` passing its conformance; units of the simulated plant; `Stale` before `CommFailure`; `g` → `gn`; ADS1115 with shunt | 01 §21, 11, 14 §7, 04 §6, old track 04, 16 §5, example 00 |
| status of the ADRs | all become "accepted (v1)"; new ADR 0018 | adr/ |
| precedence | explicit order between documents; examples and track are illustrative | docs/README, AGENTS |

---

## 3. What remains open (and why it does not block)

| Topic | When it is decided | Why it does not block Phase 0 |
|---|---|---|
| exact formula of RT admission, `dt()` after `skip`, alarm transitions from `Unknown`, bus budget beyond RTU, base of `extends` paths | ADRs during Phase 0 (document 16, §10) | each one belongs to a concrete task and has a reasonable default |
| exact replay of async tasks (`trace.async`) | Phase 2 | Phase 0 only records the cycle domain |
| `jk class extract` from crates | after v1 | local classes in crates already work inside apps |
| methods as pure Wasm functions | only with a real case | extension traits and active implementations cover the known cases |
| figures of the TS and Python SDKs | Phase 3 (TS) and 1.2 (Python) | tier 2 |
| the six Burn validations | Phase 2 exit gate | ML starts in Phase 2 |
| `jk lsp` with expressions embedded in the TOML | Phase 1 | it is DX, not semantics |
| fleet server | the Joker Deck (document 20, ADR 0021; document 12, §7) | — |

---

## 4. The risks at the start of implementation

| # | Risk | Warning sign | Mitigation already in the plan |
|---|---|---|---|
| 1 | **Scope.** 24 primitives, one IDL and 18 documents are a lot for a team of 3–4 people | Phase 0 goes beyond 8 weeks; Phase 2 tasks creeping into 1 | phases with exit gates; decision point after Phase 1; optional crates by feature (document 09, attack 12) |
| 2 | **Real time yet to be proven.** The jitter targets are estimates | Phase 1 does not reach 2× the target | measure per board from Phase 0 (`xtask rt-bench`); the promise drops to groups ≥ 10 ms if it fails |
| 3 | **Cost of profiles and drivers.** The competitive moat is also the largest cost | profiling a PCS takes more than a week | importers (SunSpec, CSV) and `jk profile test` in Phase 1; explicit metric at the decision point |
| 4 | **Wasm in motion.** WASI 0.3 is recent, and the Component Model keeps evolving | a wasmtime version breaks the WIT world | wasmtime version pinned per runtime minor; M21 policy; adapters |
| 5 | **Burn pre-1.0** | breaking changes between minors | pinned version; ORT as a second backend from Phase 2; ADR 0009 revised by the validations |
| 6 | **Embedded 0.x toolchains and dependencies** (Embassy, `embedded-io`, `esp-hal` `unstable`, Xtensa toolchain outside official LLVM; document 25, §5) | a `cargo update` breaks one target and not another; the `esp` toolchain lags behind stable Rust | one `rust-toolchain.toml` per family (official and `esp`); exact versions of the 0.x dependencies; update once per phase, with conformance as the gate; Xtensa is never the only target and does not block CI (document 13, §5 and §5.1) |

---

## 5. Freeze checklist

- [x] Every architecture decision has an accepted ADR (0001–0018).
- [x] The core semantics are specified at the level of conformance cases (document 16).
- [x] The class IDL is specified, with grammar, execution and versions (document 17).
- [x] The TOML examples of the documents are syntactically valid.
- [x] There are no known contradictions between normative documents, and the precedence order is written down.
- [x] The Phase 0 plan has tasks with dependencies and references to the specification (document 13).
- [x] The limits are declared (document 12).
- [x] The crate names have been checked.
- [x] License chosen: Apache-2.0 OR MIT (2026-10-21).
- [ ] Name and trademark confirmed.
- [ ] Team and front owners defined (AGENTS.md, §3).
- [ ] CI hardware (CM4, CM5, RevPi Connect 5, x86 PC) available or ordered.

The four unchecked items **are not technical** and belong to the project owner.

---

## 6. Verdict

**Yes: the documentation may be frozen as v1, and Phase 0 may start.**

- Phase 0 is the core without I/O, and it is now fully specified by documents 16, 01, 02 and 03, with a conformance schema that an independent team can write without reading code.
- What is not yet decided is listed, has an owner and has a time (§3), and none of it is needed for the first 21 tasks.
- The missing decisions are four, and none is technical:

| Decision | Recommendation | Why |
|---|---|---|
| **License** | Apache-2.0 OR MIT (dual), like Rust, wasmtime and Burn | compatible with the whole ecosystem Joker depends on; if the intent is open core, decide **now** which crates stay closed, before the first public commit |
| **Name** | keep "Joker" as the project name and `joker-*` for the crates | the crates are free; the trademark check should be requested from a lawyer before any public launch |
| **Team** | one person per Phase 0 front: core, manifest, conformance (separate from the core, by the Cravo rule) | conformance is only worth something if whoever writes it does not write the core |
| **CI hardware** | 1× CM4, 1× CM5, 1× RevPi Connect 5 and 1× x86 PC with PREEMPT_RT, from Phase 0 | the RT exit gate must be measured on real hardware from the start |

### First two weeks

1. Create the repository (jj over git) with the `AGENTS.md`, the `README.md`, the `docs/` of this v1 with the `docs-v1` tag, and the `rust-toolchain.toml`.
2. Skeleton of the workspace of document 13, §1, with the dependency rule in CI.
3. `joker-units` (task 1) and `codes.toml` (task 21), which unblock everything else.
4. In parallel, the conformance front starts the first 20 cases of document 16, §8, without seeing code.
5. `xtask rt-bench` running on a CM5 with PREEMPT_RT with an empty task: the first real figure of the project.

From here on, **changing the design means writing an ADR** (AGENTS.md, §1).

---

## 7. Addendum: scope review

After the freeze, the v1 scope changed in three points (ADRs 0019, 0020 and 0021):

| Change | Effect on readiness |
|---|---|
| **ML moves to 1.1** | reduces risk 5 (Burn pre-1.0) for v1; nothing in Phase 0 changes |
| **Joker Micro in v1** (Phase 2) | Phase 0 gains a requirement that **must** be met from the first line: the `no_std` core, without allocation, with fixed capacities, with CI on `thumbv8m.main-none-eabihf` and `riscv32` (conformance in QEMU) and on the bench ESP32, and on an RP2350 when a board is available (document 13, task 22 and §5) |
| **Family and Deck** | the agent protocol and the Deck MVP enter Phase 3; nothing in Phase 0 changes |

**The verdict stands:** v1 may be closed and Phase 0 may start, now with the `no_std` rule in `AGENTS.md`. To the four decisions of §6 a fifth is added, tied to the license: **the business model of the family** (document 20, §6). The CI hardware now includes an RP2350 with a debug probe from Phase 0.

## 8. The next round

After this readiness, all interfaces were closed in normative references (`docs/ref/`, ADR 0023), and Phase 0 gained three initial tasks that compile and validate them in code (0a–0c). What was decided, the conflicts between references and their stress are in **document 22**.
