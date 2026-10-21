# 13 — Implementation plan

How to get off the drawing board: the workspace, the dependencies between crates, the work fronts and the first Phase 0 tasks, in order.

---

## 1. Workspace

```
Cargo.toml                 workspace; edition 2024; MSRV pinned in rust-toolchain.toml (one per target family, §5.1)
crates/
  joker-units/             units, dimensions, quantities, parsing of "125 kW"       (no deps)
  joker-core/              registry, slots, image, arbiter, envelope, alarms, trace (← units, expr)
  joker-expr/              expression language → bytecode                           (← units)
  joker-manifest/          joker.toml, joker.lock, overlays, J-codes, codes.toml, RT admission  (← core, units, expr)
  joker-hal/               Planner, Driver and RtPart traits, plans, transports, Cx (ref/06 §3)  (← core)
  joker-drivers/sim/       simulated plant                                          (← hal)
  joker-drivers/modbus/    tcp + rtu + sunspec                                      (← hal)
  joker-drivers/gpio/      chardev v2                                               (← hal)
  joker-drivers/revpi/     piControl                                                (← hal)
  joker-drivers/mqtt/      MQTT input                                               (← hal)
  joker-driver-sdk/        SDK for tier W drivers in Rust (export_driver!)          (← hal)
  joker-jdp/               JDP: protocol, rings, client and SDK for tier X drivers  (← hal)
  joker-drvtest/           driver conformance suite and equipment doubles           (← hal)
  joker-host/              wasmtime, WIT world, capabilities, AOT                   (← core)
  joker-sdk/               Rust SDK (guest), macros in joker-sdk-macros             (← WIT)
  joker-test/              harness with virtual clock                               (← core, host, sim)
  joker-store/             historian and cursors                                    (← core)
  joker-link/              MQTT/Sparkplug, then Zenoh and OTLP                      (← core, store)
  joker-api/               HTTP + WebSocket (ref/09)                                (← core)
  joker-client/            Rust client library for the API; jk uses it (ref/05)     (← spec/openapi)
  joker-agent/             node ↔ Deck agent, agent/1 (ref/10)                      (← agent-proto, core)  (Phase 3)
  joker-agent-proto/       agent messages in CBOR, no_std, shared with Micro and the Deck  (Phase 3)
  joker-micro/             runtime for MCUs (Embassy): no_std host of the core, firmware/C lib/crate modes; ffi module with the C ABI  (Phase 2)
  joker-link-proto/        Joker Link protocol, no_std, shared by jokerd and joker-micro  (Phase 2)
  joker-jkc/               .jkc class compiler (parser, checking, canonical form, codegen)  (← expr, units)
  joker-ml/ joker-ml-burn/ joker-ml-ort/ joker-ml-hailo/                            (1.1)
  joker-spec-tests/        task 0a: compiles and validates the references against the code (document 22, §8)
  joker-supply-chain/      tests that read the dependency graph and the toolchain files (policies/dependencies.md §1)
  jokerd/                  daemon binary
  joker-cli/               CLI crate; the binary is called `jk` (the name `jk` is taken on crates.io)
wit/                       joker-runtime.wit, joker-driver.wit, joker-sim.wit
spec/                      cddl/, openapi/, c/: the machine-readable files of the references (ADR 0023)
conformance/               cases as data (TOML), one file per suite: conformance/<area>/<suite>.conformance.toml
xtask/                     rt-bench, conformance runner, release
```

**`no_std` rule** (ADR 0020; **this is the normative list**, and ADR 0020, document 19 and AGENTS.md refer to it): `joker-units`, `joker-expr`, `joker-core`, `joker-link-proto` and `joker-agent-proto` are `#![no_std]`, with no allocation on the tick path; CI builds them for `thumbv8m.main-none-eabihf` and `riscv32imac-unknown-none-elf` on every commit (and for Xtensa, without blocking; §5). `joker-hal`, `joker-driver-sdk` and `joker-micro` are also `no_std`, with `alloc` and `std` as features (ref/06 §3.1, ref/08 §2.11). Synchronization in the core is only through `critical-section` and `portable-atomic` (ref/08 §2.4, rule 8).

**References and code change together** (ADR 0023): each front owns the references of its surface (§2), and the CI of task 0a fails if a block of a reference or a file in `wit/` or `spec/` diverges from the code.

**Dependency rule** checked in CI (`cargo deny`, `cargo vet` and the tests of `joker-supply-chain`, which read the graph of `cargo metadata`): `joker-core` depends only on `joker-units` and `joker-expr`; no domain crate depends on `jokerd`; the `no_std` crates above depend only on `no_std` crates. The complete list of rules, the allowed crates and the critical-dependency table are in `policies/dependencies.md` (ADR 0055).

---

## 2. Fronts

| Front | Crates | References it owns | First milestone |
|---|---|---|---|
| Core | `joker-units`, `joker-expr`, `joker-core` | 16 and ref/00 (with all fronts reviewing) | envelope + arbiter with conformance |
| Manifest | `joker-manifest` | ref/01; ref/02 §2.1 and §3.1 (lock) | `jk check` over the examples in this repository |
| Host | `joker-host`, `joker-sdk`, `wit/` | ref/04; ref/05 part A (tier 2 SDKs) | example app running in the harness |
| Drivers | `joker-hal`, `joker-drivers/*`, `joker-driver-sdk`, `joker-jdp`, `joker-drvtest` | ref/06; ref/03 §2.2–§2.4 (profiles, boards, simulation) | Modbus TCP against a simulator |
| Services | `joker-store`, `joker-link` | — (document 01, §12–§13) | historian with WAL and retention |
| Daemon, API and CLI | `jokerd`, `joker-api`, `joker-client`, `joker-cli` | ref/09; ref/02 §2.2–§2.5 (node, client, secrets, trust); ref/05 part B (clients) | `jk dev` with simulated plant |
| Joker Micro and Link (Phase 2) | `joker-micro`, `joker-link-proto` | ref/07, ref/08 | core + link on an RP2350 |
| Agent (Phase 3) | `joker-agent`, `joker-agent-proto` | ref/10 | enrollment and desired state against a test Deck |
| Classes | `joker-jkc`, `classes/std/` | 17; ref/03 §2.7 (packages) | the initial `std/` (document 14, §10) passing its conformance |
| Conformance | `conformance/`, `joker-spec-tests` | ref/03 §2.5–§2.6 (scenarios and conformance schema); tasks 0a and 0c | 150 cases, written by people who do not write the core |

The conformance front writes the cases **from the spec**, without reading the core's code. This is the Cravo rule: sharing code is sharing bugs.

---

## 3. Format of the conformance cases

There is a single schema for the core and for the classes (`[suite]`, `[[case]]`, `do`, `expect`, `at`, `equals`, with the canonical literals in lowercase), defined in ref/03 §2.6, from the rules of document 16, §8. There is **one file per suite**, `conformance/<area>/<suite>.conformance.toml`, with `schema = 1` as the first pair. The runner (`cargo xtask conformance`) runs each case against `joker-core` with a virtual clock and compares result by result. The expected values are written by hand from the specification. The same files run on an MCU through the conformance firmware (ref/08 §3.2, task 22).

---

## 4. The first Phase 0 tasks

| # | Task | Front | Depends on |
|---|---|---|---|
| 0a | `joker-spec-tests`: doctests of the Rust blocks of the references, `tsc` on the TypeScript blocks, CDDL validated against the examples, JSON schema of the manifest generated from the types and validated against every TOML block, OpenAPI validated, counts in the prose checked against the sources (§5.1); fails CI if reference and code diverge | Conformance | — |
| 0b | `codes.toml`: all the codes of the references (the seed `spec/codes.toml`, which is the source of the counts) in a single registry, and the CI test that checks it | Manifest | — |
| 0c | test vectors for Joker Link (ref/07 §4.8), for the C ABI (ref/08) and core conformance cases as files in `conformance/`, run on Linux and on an MCU | Conformance | — |
| 1 | `joker-units`: the catalog of document 16, §5 (dimensions, prefixes, affine `°C`/`K`, quantities), parsing of literals | Core | — |
| 2 | Conversion table and tests with UCUM cases | Core | 1 |
| 3 | `joker-expr`: lexer and parser with spans (for rustc-style errors) | Core | 1 |
| 4 | `joker-expr`: type and unit checking (`J0301`…) | Core | 3 |
| 5 | `joker-expr`: bytecode and allocation-free stack VM; window functions | Core | 4 |
| 6 | `joker-core`: `Slot`, seqlock and tests with loom | Core | — |
| 7 | `joker-core`: immutable registry and dense `SignalId`s | Core | 6 |
| 8 | `joker-core`: per-group image (latch, staging) | Core | 7 |
| 9 | `joker-core`: 16-level arbiter with leases and manual TTL | Core | 8 |
| 10 | `joker-core`: envelope (steps 0–8) and trips | Core | 5, 9 |
| 11 | `joker-core`: atomic groups with proportional rate | Core | 10 |
| 12 | `joker-core`: alarms (state machine, delay, shelving) | Core | 5 |
| 13 | `joker-core`: trace in the format of document 16, §7, and causal tracing (bitset of reads) | Core | 8 |
| 14 | `conformance/`: runner for the schema of ref/03 §2.6, and the first 50 cases (arbiter A1–A14, envelope E1–E10, quality §3) | Conformance | spec |
| 15 | `joker-manifest`: parsing, model, J-codes with spans | Manifest | 1, 4 |
| 16 | `joker-manifest`: overlays (`extends`) and an envelope that only tightens | Manifest | 15 |
| 17 | `joker-manifest`: RT admission (rate-monotonic) and bus budget | Manifest | 15 |
| 18 | `joker-drivers/sim`: generators, dynamics, faults | Drivers | 7 |
| 19 | `joker-test`: harness with virtual clock (still without Wasm: native test tasks) | Host | 10, 18 |
| 20 | `xtask rt-bench` and no-alloc guard on the tick path | Core | 10 |
| 21 | `codes.toml`: loaded by `joker-manifest`, with the J-codes of document 16, §9, and those of task 0b | Manifest | 0b, 15 |
| 22 | CI: `no_std` build for `thumbv8m.main-none-eabihf` and `riscv32imac-unknown-none-elf` with conformance in QEMU (blocking), for `xtensa-esp32-none-elf` with conformance on the bench ESP32 (non-blocking), and on an RP2350 via probe once a board is available (§5) | Core | 6, 14 |

---

## 5. CI

| Job | Where | When | Blocks |
|---|---|---|---|
| fmt, clippy, tests | x86_64 | every commit | yes |
| supply chain: `cargo deny`, `cargo vet --locked`, dependency graph and toolchain files (`joker-supply-chain`; `policies/dependencies.md` §1) | x86_64 | every commit | yes |
| references (`joker-spec-tests`: doctests, `tsc`, CDDL, manifest schema, OpenAPI, counts from §5.1) | x86_64 | every commit | yes |
| conformance | x86_64 | every commit | yes |
| long proptest (10⁶) | x86_64 | nightly | — |
| cross-build aarch64, armv7, `arm-unknown-linux-gnueabihf` (ARMv6), riscv64 | x86_64 (cross) | every commit | yes |
| `no_std` cross-build **`thumbv8m.main-none-eabihf`** and **`riscv32imac-unknown-none-elf`**, and conformance in QEMU (`mps2-an505`, `virt` riscv32) | x86_64 (cross) | every commit | **yes** |
| size of `jokerd` (`minimal` and full, `cargo bloat`, 01 §21) | x86_64 (cross aarch64) | every commit | yes |
| **Xtensa** cross-build (`xtensa-esp32-none-elf`, `esp` toolchain) and conformance on the bench ESP32 | runner with the `esp` toolchain | every commit in the core | **no**: the result shows up in the PR and opens an alert, but does not prevent the merge |
| conformance on an RP2350 (HIL) | runner with probe | every commit in the core, once a board is available | yes |
| hardware tests (CM4, CM5, RevPi Connect 5, x86 IPC) | dedicated runners | nightly, and on PRs with the `hw` label | — |
| `rt-bench` with PREEMPT_RT | dedicated runners | nightly; regression > 5% in p99.9 fails | yes |
| HIL bench (Phase 1+) | BESS bench | nightly | — |

**Why Xtensa does not block.** The `esp` toolchain (`espup`) is outside official LLVM and follows stable Rust with a lag; a delay on its side must not stop the core. Reference conformance runs on Cortex-M33 and RISC-V with official toolchains, and Xtensa is one more target, never the only one (document 25, §5). A failure on Xtensa is fixed before the end of the phase.

### 5.1 Toolchains and MSRV

| Rule | Why |
|---|---|
| **One `rust-toolchain.toml` per target family:** the root one (official toolchain: Linux, `thumbv8m`, `riscv32`) and one in `crates/joker-micro/esp/` (`esp` toolchain, installed by `espup` with a version pinned in a CI image) | the two families have different release schedules; with a single file, the slower one would hold back the other |
| **The MSRV of the core** (`joker-units`, `joker-expr`, `joker-core`, `joker-link-proto`, `joker-agent-proto`) is **≤ the Rust version of the most recent Xtensa toolchain**; the other crates may require more | the core must build for the bench ESP32 from the same source |
| **The 0.x dependencies** (Embassy, `embedded-io`, `esp-hal` `unstable`, `esp-radio`, `probe-rs`) have an exact version in the workspace `Cargo.lock`, and appear only in the hosts and in the tools, never in the core | an upstream breakage reaches neither the core nor the C ABI |
| **Updating toolchains and 0.x dependencies is an explicit task, once per phase**, with conformance (Linux, QEMU and boards) as the gate | drip-fed updates break a target without anyone noticing |
| **Xtensa is never the only target** of a conformance check | it is the third-party toolchain; see the previous paragraph |

The `toolchain` test of `joker-supply-chain` checks the first two rows, and the per-phase update and the wasmtime line are in `policies/dependencies.md` §5–§7 (ADR 0055).

The counts that the prose still repeats (codes, API operations, Joker Link messages) are checked by `joker-spec-tests` (task 0a) against the sources: `crates/joker-manifest/codes.toml`, the OpenAPI and the CDDL. Where it is easy, the prose refers to the source instead of repeating the number.
