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
| 5 | 10 minutes recorded on the Pi 3 replay on the laptop with the same bytes, 100 times | **Fixed (2026-10-22)** | 10 minutes recorded live on a real Pi 3 B+ (`jk record --for 10m` over `ssh://`), replayed 100 times on the dev laptop (`jk replay`): all 100 runs byte-identical (single md5 across all 100 JSON reports), 0 divergences, 29999/29999 ticks, state restored from the recording (not `init`) — see the 2026-10-22 update below |
| 6 | 30+ conformance cases from the spec only pass on x86_64 and aarch64; core compiles for `thumbv8m` and `xtensa-esp32` | **Fixed** | 152 cases (`conformance/`), CI green on x86_64, aarch64, QEMU thumbv8m/riscv32; Xtensa build green (non-blocking job, as designed) |
| 7 | `jk check` unit error comes out as in document 00 | **Fixed** | `joker-manifest` snapshot tests against the registry messages (`J0107` and siblings) |
| 8 | First real figures (jitter on Pi 3, size, RSS, startup, Wasm call cost, build time) | **Partial — x86_64 and Pi 3** | See the table below and the 2026-10-22 update. Pi 3 B+ figures now measured for real (jitter, binary size, RSS, startup); Cortex-M33 and the Wasm call cost on Pi 3 are still open — see F-2 |
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

## 2026-10-22 update — real Raspberry Pi 3 B+ results (gates 5 and 8)

Evidence gathered on 2026-10-22 against a real Raspberry Pi 3 B+ (the owner's own bench unit, not CI), reachable over SSH, running `jokerd` cross-built for `arm-unknown-linux-musleabihf` (a static musl binary targeting ARMv6 hardfloat, which runs unmodified on the Pi 3's ARMv7 core — ARMv6 code is a strict subset). OS: Raspbian GNU/Linux 9 (stretch), kernel `4.19.66-v7+`, stock (no PREEMPT_RT, no `isolcpus`). The bench ESP32 was not powered on in this pass (gates 1's Wasm leg and gate 2 are still open — see F-1); this update closes gate 5 and the Pi 3 half of gate 8.

**Gate 5 (record → replay, byte-identical, 100 times).** `jk record --for 10m --out trace.bin` against the live Pi 3 node over `ssh://pi@<host>` (the client's existing `-L` unix-socket forwarding, ADR-free — no code change needed), then `jk replay trace.bin` **100 times** on the dev laptop:

| Figure | Result |
|---|---|
| Ticks replayed | 29999 / 29999 (10 min @ 20 ms) |
| Divergences | 0, on every one of the 100 runs |
| App state | restored from the recording (`"restored": ["light"]`, not `from_init`) |
| Byte-identical across 100 runs | Yes — `md5sum` of all 100 `jk replay --json` outputs collapses to a single hash |

Workload: the `light` app (native stand-in, see the note on gate 1 below) bound to a real `sysfs` output (the Pi's own ACT LED, `led0`), blinking at a 500 ms half-period, 20 ms cycle.

**Gate 8, Pi 3 half.** `cargo xtask rt-bench` cross-built and run directly on the Pi 3 (release, stripped):

| Figure | 1 ms period | 100 ms period | Target (docs/01 §21) |
|---|---|---|---|
| Envelope + arbiter per output, p50 | 9.44 µs | 9.22 µs | ≤ 300 ns |
| Envelope + arbiter per output, p99.9 | 13.85 µs | 13.61 µs | ≤ 300 ns |
| Tick jitter, p50 | 75.3 µs | 88.5 µs | — |
| Tick jitter, p99.9 | 109.8 µs | 3524.3 µs | ≤ 2 ms (no RT, 100 ms group) |
| Tick jitter, max | 241.1 µs | 3524.3 µs | — |
| Overruns | 7 / 10000 | 0 / 600 | — |

| Figure | Value | Target |
|---|---|---|
| `jokerd` binary size (release, stripped, musl static) | 5.0 MB (5,225,368 B) | ≤ 20 MB minimal (aarch64 target; not directly comparable arch) |
| Idle RSS (1 signal, 1 app, no historian) | ~30 MB | ≤ 60 MB (measured under a much lighter load than the target's own condition — 1 output vs. 1,000 signals/3 apps/8 MB historian cache, so not apples to apples) |
| Startup to first tick (`--manifest`, cold) | 332 ms | ≤ 2 s |
| Cross-build time, `jokerd` release (`arm-unknown-linux-musleabihf`, on the dev x86_64 machine, 4 cores busy) | 2 min 10 s | — |
| Cross-build time, `xtask` release (same target) | 10.9 s | — |
| Wasm call cost on Pi 3 | **not measured** | `joker-host` (wasmtime/Cranelift) was not cross-built for this target: Cranelift has no stable ARMv6/v7 (32-bit) backend (docs/01 §22 already notes armv7 needs Pulley, not Cranelift, and ARMv6 wasmtime support is unconfirmed); `joker-host` is also not wired into `jokerd` yet regardless (see `jokerd::native::NativeTasks`'s own doc comment) |

**Reading it.** The per-output envelope+arbiter cost (9–14 µs) is roughly 30–45× the ≤300 ns target — expected on a 2015-era Cortex-A53 at 1.4 GHz with no atomic-group fast path yet, and not the hardware the target line is calibrated for (CM4/CM5). The 100 ms jitter p99.9 (3.52 ms) is over the ≤2 ms non-RT target, but **this run was not isolated**: the SSH session used to launch it and poll results was active on the same machine throughout, the same confound F-2's x86_64 numbers already called out. A clean rerun (nothing else connected, ideally via a serial console or a fire-and-forget `nohup` with no polling) is needed before treating this as a real divergence worth an erratum. Startup time, binary size and RSS are all comfortably inside target even on this older, weaker board.

**Resolution.** Gate 5: resolved. Gate 8: still partial — the Cortex-M33 figures and a clean (unloaded) Pi 3 jitter run are open; the Wasm call cost needs either a Pulley backend build for `arm-unknown-linux-musleabihf` or waiting for a 64-bit (`aarch64`) Pi image, which is also the planned fix for AUR-401's real GPIO/I²C adapter (the chardev v2 ioctls it needs are Linux ≥5.10 only; this Pi's stock kernel is 4.19). Gates 1 (the Wasm leg specifically — this pass used the native `light` stand-in, not an actual `.wasm` component) and 2 remain open, blocked on the bench ESP32 being wired and powered per document 23 §4–§7.

## Other observations (non-blocking)

- **O-1 — local toolchain drift is a trap, not a product bug.** This machine's ambient Rust (nix `languages.rust.channel = "nightly"` in the untracked `devenv.nix`) does not match `rust-toolchain.toml` (`1.99.0` with `thumbv8m.main-none-eabihf`, `riscv32imac-unknown-none-elf`, `wasm32-wasip2`) and shadows it on `PATH`, so every test that builds a Wasm component failed locally with `can't find crate for core` until the pinned toolchain was installed explicitly (`rustup target add wasm32-wasip2 …` under `~/.rustup`, bypassing the nix-provided `rustc`). CI is unaffected (it uses the pinned toolchain directly) and the project needs no change; worth a one-line note in `AGENTS.md` or a repo `README` for anyone using a Nix-based personal devenv on top of the project's own toolchain file.
