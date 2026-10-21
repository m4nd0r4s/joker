# Dependency, toolchain and wasmtime policies

**Status:** normative for the repository (ADR 0055); ranks with the ADRs (`docs/README.md`). It answers document 26 §4 (E2, E8, E9) and §9, and makes document 13 §1 and §5.1 checkable. A rule changes here and in its check in the same commit.

---

## 1. What CI enforces

Every row is a blocking job on every commit (document 13 §5).

| Rule | Source | Checked by |
|---|---|---|
| `joker-core` depends, among the workspace crates, only on `joker-units` and `joker-expr`; `joker-expr` only on `joker-units`; `joker-units` on none | 13 §1 | `joker-supply-chain`, `graph` test |
| no workspace crate depends on `jokerd`, with any kind of dependency | 13 §1 | `graph` test |
| the `no_std` crates of 13 §1 depend only on `no_std` crates of the workspace and on the `no_std` crates of §3, without their `std` feature; a dependency that only the crate's own `std` feature turns on is outside the rule | ADR 0020, 13 §1 | `graph` test, and the `no_std` cross-build |
| the core crates (`joker-units`, `joker-expr`, `joker-core`, `joker-link-proto`, `joker-agent-proto`) depend only on the core allowlist of §3 | ref/08 §2.11, 25 §5 | `graph` test |
| every external crate reachable from the `no_std` crates without `std` is available under MIT or Apache-2.0 (§8) | 26 E9 | `graph` test |
| workspace licence allowlist, banned and duplicate crates, RustSec advisories, only crates.io as a source | 26 E8, E9 | `cargo deny check` (`deny.toml`) |
| every crate version in `Cargo.lock` is audited, trusted or exempted (§2) | 26 E8 | `cargo vet --locked` |
| one `rust-toolchain.toml` per target family, exact pin, the workspace `rust-version` is its minor, the core MSRV is at most the `esp` toolchain (§5) | 13 §5.1 | `toolchain` test |
| the wasmtime version of `joker-host` is the one in `Cargo.lock` | ADR 0052 | `joker-host` tests |

The tests of `joker-supply-chain` run `cargo metadata --locked` on the workspace, so a stale `Cargo.lock` fails them too.

## 2. `cargo vet`

- **Store:** `supply-chain/` (`config.toml`, `audits.toml`, `imports.lock`). The header comments are written and required by `cargo vet`, as in `Cargo.lock`.
- **Criteria:** `safe-to-deploy` for everything that ships; `safe-to-run` for development dependencies (the default policy of `cargo vet`).
- **Imports:** the audit sets of the cargo-vet registry that cover our graph: Bytecode Alliance (wasmtime and its dependencies), Google, Mozilla, ISRG, Zcash and Ariel OS (embedded). `imports.lock` is committed and CI runs with `--locked`, so a check never depends on what an upstream set says today.
- **Trust:** a publisher is trusted for a crate (`cargo vet trust`) only when at least two imported sets already trust that publisher. A trust entry lasts one year and is renewed in the per-phase update (§5).
- **Exemptions:** the exemptions written at adoption (2026-10-21) are a frozen baseline. A new crate, or a new version of a crate, is covered by an import, by a trust entry under the rule above or by an audit of ours (`cargo vet certify`). A new exemption needs its reason in the commit body. The per-phase update runs `cargo vet prune` and tries to remove exemptions.
- **The core is audited, not exempted.** `libm` is trusted (Rust project). `critical-section` and `portable-atomic` are still in the baseline; they get an audit of ours before the end of Phase 0, and every later version is audited as a delta.

## 3. Allowed `no_std` crates

| Crate | Line | Core allowlist | Why |
|---|---|---|---|
| `critical-section` | 1.x | yes | the only synchronization of the core, with `portable-atomic` (ref/08 §2.4, rule 8) |
| `portable-atomic` | 1.x | yes | 64-bit atomics where the target has none |
| `heapless` | 0.9 | yes | fixed-capacity collections; trivial |
| `minicbor` | 2.x | yes | allowed by ref/08 §2.11; the link keeps its own codec (26 E8) |
| `libm` | 0.2 | yes | pure maths for `joker-expr`; maintained by the Rust project (ADR 0055) |
| `embedded-io`, `embedded-io-async` | 0.7 | no | transports of `joker-hal`; 0.x, outside the core (13 §5.1) |

Adding a crate to this table is a change of this document and of the constants of the `graph` test in the same commit. Being in the table allows a dependency; it does not choose one.

## 4. Critical dependencies and fork plans

A dependency is critical when it is on the cycle path, in the core, in what a node runs, or when a single person can block it (26 E8). For each, what happens if it is abandoned, relicensed or compromised.

| Crate | Where | Maintainers | Risk | Plan |
|---|---|---|---|---|
| `wasmtime`, `wasmtime-wasi`, `cranelift-*` | `joker-host` (every app) | Bytecode Alliance, many, LTS lines | security bugs in the sandbox (E2) | follow the LTS line (§7); defence in depth: seccomp and Landlock in `jokerd`, the `safe` of each output; a fork is not realistic, the plan is to stay on a supported line |
| `wasmparser`, `wit-bindgen` | `joker-host`, `joker-sdk-sys` | Bytecode Alliance | breaking changes between majors | same line as wasmtime; wit-bindgen pinned exactly, updated per phase |
| `portable-atomic` | core | essentially one person (Taiki Endo) | single maintainer, in the core | small, pinned, audited (§2); fork under the project if it is abandoned; the fallback is our own `critical-section` cell for the few 64-bit atomics the core uses |
| `critical-section` | core | Rust Embedded working group | low | pinned 1.x; the trait is small enough to vendor |
| `libm` | `joker-expr` | Rust project | low | pinned; the few functions used can be vendored from musl |
| `heapless` | core (when used) | Rust Embedded working group | 0.x breakage | pinned; vendor the used types if needed |
| `minicbor` | allowed, unused | one person | single maintainer | not used by the link (own codec, E8); if adopted, pinned and audited |
| `embedded-io`, `embedded-io-async` | `joker-hal` | Rust Embedded working group | 0.x breakage | kept out of the core and of the C ABI; one version in the workspace |
| `tokio` | `joker-hal` (`std`), drivers | tokio project, many | low | pinned 1.x; only in the asynchronous domain |
| `rustix`, `linux-raw-sys` | `joker-hal` (`std`), wasmtime | Bytecode Alliance, one main author (Dan Gohman) | single main maintainer | used through `joker-hal` only; `libc` (Rust project) is the fallback for the few calls we make |
| `libc` | `joker-serial-sys` | Rust project | low | pinned |
| `toml` | drivers, manifest | `toml-rs`, one main author (Ed Page) | single main maintainer | pinned 1.x; the manifest crate owns its schema, so a parser swap does not change a file format |
| `sha2`, `rand_core` | `joker-host` | RustCrypto, rust-random | low | pinned; `sha2` has drop-in alternatives |

The table is reviewed in the per-phase update (§5), with `cargo vet suggest` and the crates.io owners of each crate.

## 5. Toolchains

- **One `rust-toolchain.toml` per target family** (13 §5.1): the root one (official toolchain: Linux, `thumbv8m`, `riscv32`, `wasm32-wasip2`) and `crates/joker-micro/esp/rust-toolchain.toml` (`esp` toolchain, installed by `espup`), which arrives with `joker-micro`. No other toolchain file exists.
- **Exact pin.** The root channel is an exact version (`1.99.0`), and the workspace `rust-version` is its minor (`1.99`).
- **Core MSRV ≤ Xtensa.** The core MSRV is at most the Rust version of the most recent `esp` toolchain. On 2026-10-21 that is 1.99 (`esp` 1.99.0.0, published 2026-09-30), so the root stays at 1.99.0. The `toolchain` test holds that version (`XTENSA_RUST` in `joker-supply-chain`) and fails when a core crate's `rust-version` is above it; the per-phase update changes it together with the version that `espup` installs in the CI image.
- **Once per phase.** Updating the toolchains and the 0.x dependencies is an explicit task at the start of each phase (an issue in Linear): both toolchain files, `cargo update` of the 0.x crates, `cargo vet` (audit the deltas, prune, renew trust entries), `cargo deny`, then the gate: conformance on Linux, in QEMU (`mps2-an505`, `virt` riscv32) and on the boards. It merges only green.
- **Out of band.** A RustSec advisory (`cargo deny` fails) or a wasmtime security release (§7) is fixed outside the cadence, with the smallest version change that fixes it.

## 6. 0.x dependencies

- Exact version in `Cargo.lock`, which is committed; updated only by the per-phase task or out of band (§5).
- Never in the core (§3); in the hosts and the tools, behind modules that can be updated without touching the core or the C ABI (25 §5).

## 7. wasmtime

- **Line.** Wasmtime publishes a major every month. Every twelfth major (24, 36, 48, 60…) is a long-term support line, with security fixes for 24 months; any other major gets them only while it is one of the two most recent. Joker uses the most recent LTS line: **48** in Phase 0 (published 2026-08-20, supported until August 2028), pinned exactly (`=48.0.N`) in the workspace, with `wasmtime-wasi` and the `cranelift-*` crates on the same line.
- **Patches of the line.** A patch that fixes a security advisory (the GitHub advisories of `bytecodealliance/wasmtime`, and RustSec through `cargo deny`) is taken within **7 days** when it is critical or high and touches a feature we enable, and within **30 days** otherwise. The other patches go in with the per-phase update.
- **The next line.** Moving to the next LTS line (60, due in August 2027) is the per-phase task of the phase in which it is published, and never later than six months before the current line loses support. A non-LTS major is only used with an ADR that names the feature it is for and the date of the return to an LTS line.
- **AOT.** The engine identity contains the wasmtime version and the target triple (ADR 0052), so an update invalidates every AOT cache by construction, and a node in `prod` never compiles (`J-E0010`). The `.cwasm` of each board triple is built by `jk build --target` and by CI; that CI job lands with `jk build` (document 07).

## 8. Licences

- The project is Apache-2.0 OR MIT (document 18 §5).
- **Workspace allowlist** (`deny.toml`): Apache-2.0, Apache-2.0 WITH LLVM-exception, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib.
- **MCU set** (narrower): every external crate reachable from the `no_std` crates without their `std` feature must be available under MIT or Apache-2.0 (one of the alternatives of its licence expression), so that a firmware or the C library ships under the project's own terms.
- **Copyleft** only behind a tier X driver, across a process boundary (26 E9).
