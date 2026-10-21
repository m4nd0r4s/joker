# M0 friction report (October 2026)

Gate 9 of the first vertical slice (docs/26 §6): every divergence between the specification and reality found while closing M0, recorded here before it becomes an erratum or an ADR. Not normative. Evidence was gathered on 2026-10-21 against `main` (9d270d3, PR #61 integrated), on a development machine (x86_64, 12 cores), with the project's pinned toolchain (`rustc 1.99.0`, targets `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `wasm32-wasip2`).

Each entry has a status:

- **Fixed:** resolved in this pass.
- **Resolved:** a decision was made and applied; nothing left open.
- **Open:** needs the project owner, usually because it needs hardware this environment does not have.

## Gate-by-gate status (docs/26 §6)

| # | Gate | Status | Evidence |
|---|---|---|---|
| 1 | Same `light.wasm` runs in the simulator and on the Pi 3, only the manifest changes | **Open (hardware)** | `crates/jokerd/tests/bench_example.rs` proves it end to end against the simulator on this machine (gate 5 code path); the Pi 3 leg is unverified here — see F-1 |
| 2 | ESP32 cable unplugged → `bad(comm_failure)`, LED off via firmware, app sees `Err` | **Open (hardware)** | `unplugged_cable_gives_comm_failure_within_poll_plus_timeout` (`crates/joker-drivers/modbus/tests/bench_esp32.rs`) proves the logic against a serial double; no run against the real ESP32 bench firmware — see F-1 |
| 3 | Manual write with TTL beats the app and expires by itself | **Fixed** | `joker-core` arbiter unit tests (A1–A13) and `conformance/arbiter/*` (22 cases) pass |
| 4 | Generation that breaks the bus reverts if not confirmed | **Fixed** | `an_unconfirmed_generation_is_rolled_back`, `a_crash_while_confirming_boots_the_previous_generation` (`crates/jokerd/tests/generations.rs`) |
| 5 | 10 minutes recorded on the Pi 3 replay on the laptop with the same bytes, 100 times | **Open (hardware)** | `crates/jokerd/tests/replay.rs` and `bench_example.rs` prove byte-identical record→replay on this machine, once, not on the Pi 3 and not 100 times — see F-1 |
| 6 | 30+ conformance cases from the spec only pass on x86_64 and aarch64; core compiles for `thumbv8m` and `xtensa-esp32` | **Fixed** | 152 cases (`conformance/`), CI green on x86_64, aarch64, QEMU thumbv8m/riscv32; Xtensa build green (non-blocking job, as designed) |
| 7 | `jk check` unit error comes out as in document 00 | **Fixed** | `joker-manifest` snapshot tests against the registry messages (`J0107` and siblings) |
| 8 | First real figures (jitter on Pi 3, size, RSS, startup, Wasm call cost, build time) | **Partial — x86_64 only** | See the table below. The Pi 3 and Cortex-M33 figures are the ones the gate actually asks for; this environment has no such hardware — see F-2 |
| 9 | Friction report | **Fixed** | this document |

## F-1 — Physical bench validation never ran (gates 1, 2, 5)

**Problem.** Gates 1, 2 and 5 of docs/26 §6 name the bench of document 23 by design: a real Raspberry Pi 3 B+ and a real ESP32 over Modbus RTU, with a cable physically unplugged and a trace replayed across two different hosts. Every piece of logic those gates exercise has a passing software proof (listed above), but no run has happened against the actual hardware — there is no record of it anywhere in the repository, and this development environment has no serial device attached (`/dev/serial*`, `/dev/ttyUSB*` are empty).

**Resolution.** Not resolved yet. Needs the project owner to power on the Pi 3 and the ESP32 per document 23 §4–§7 and grant SSH access, so `jokerd` can be cross-built and deployed and the three gates run for real: cable-pull, 10-minute record and 100x replay from the Pi 3 onto a laptop, and the DX walk of `jk new`/`check`/`apply`/`confirm` against the real serial link. Tracked to close in a follow-up session.

## F-2 — Gate 8 figures are x86_64 dev-machine numbers, not Pi 3 / Cortex-M33

**Problem.** The gate asks for jitter *on the Pi 3* and a per-output cost that is meaningful on the target hardware; a laptop VM with no CPU isolation is not that hardware, and the two rt-bench runs below show how much the number moves with background load alone.

**Measured (this run, x86_64, 12 cores, release build, `cargo xtask rt-bench`, default workload: 64 outputs, 1 ms period):**

| Figure | With `cargo test --workspace` running concurrently | Isolated (nothing else running) | Target (docs/01 §21) |
|---|---|---|---|
| Envelope + arbiter per output, p50 | 272 ns | 278 ns | ≤ 300 ns |
| Envelope + arbiter per output, p99.9 | 597 ns | 886 ns | ≤ 300 ns |
| Tick jitter (1 ms group), p99.9 | 3519 µs, 109 overruns | 157 µs, 0 overruns | ≤ 100 µs |
| Allocations in the tick | 0 | 0 | 0 |

| Figure | Value | Target |
|---|---|---|
| Empty on-cycle Wasm call (pack + call + check) | 821 ns | ≤ 5 µs (docs/01 §21) |
| Empty cycle task (latch + run + commit) | 1.17 µs | — |
| Clean release build, `jokerd` + `jk` (`cargo build --release`, 12-core machine) | 2 min 23 s | — |
| `jk` binary size (release, stripped) | 27 MB | — |
| `jokerd` binary size (release, stripped) | 6.2 MB | — |

`jk` is larger than `jokerd` because it embeds the daemon by design (ADR 0057), for `jk dev`/`record`/`replay`/`check`.

**Reading it.** The p50 cost per output and the Wasm call overhead are already inside their 1.0 targets even on a noisy shared VM. The p99.9 cost (886 ns isolated) is still over the 300 ns target, and jitter at p99.9 is over target even with the machine otherwise idle — expected on a non-PREEMPT_RT kernel with no core isolation, and not informative about the real target (Cortex-M33 at 150 MHz, PREEMPT_RT CM4/CM5). RSS and cold-start time were not measured in this pass — `jk dev` did not print readiness output under scripted polling on this machine and needs a person at a terminal, or a `--json`/health-check path added to `jk status` for scripted use (worth a small follow-up, not a blocker).

**Resolution.** Not resolved yet. Folds into F-1: once the Pi 3 and the bench ESP32 are reachable, rerun `cargo xtask rt-bench` on both the dev machine (already done, this table) and the Pi 3, publish both, and open an erratum if the Cortex-M33/CM4 PREEMPT_RT figures diverge from the docs/01 §21 targets by a wide margin.

## Other observations (non-blocking)

- **O-1 — local toolchain drift is a trap, not a product bug.** This machine's ambient Rust (nix `languages.rust.channel = "nightly"` in the untracked `devenv.nix`) does not match `rust-toolchain.toml` (`1.99.0` with `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `wasm32-wasip2`) and shadows it on `PATH`, so every test that builds a Wasm component failed locally with `can't find crate for core` until the pinned toolchain was installed explicitly (`rustup target add wasm32-wasip2 …` under `~/.rustup`, bypassing the nix-provided `rustc`). CI is unaffected (it uses the pinned toolchain directly) and the project needs no change; worth a one-line note in `AGENTS.md` or a repo `README` for anyone using a Nix-based personal devenv on top of the project's own toolchain file.
