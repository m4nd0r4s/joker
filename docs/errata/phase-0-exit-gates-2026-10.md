# Phase 0 exit gates (October 2026)

The five exit gates of Phase 0 (document 11), where each was validated and with what evidence, recorded for AUR-236. Not normative. The evidence is from 2026-10-25, on the stack of AUR-220, AUR-238, AUR-221, AUR-214, AUR-241 and AUR-236 over `main` at 04183bb, in GitHub Actions and on a development machine (aarch64 macOS, `rustc 1.99.0`). Without CM4, CM5 or RP2350 hardware, a gate that names hardware is validated in QEMU and on the bench of document 23, and is **provisional** until the hardware exists (ADR 0035, decision 8).

## Summary

| # | Gate | Status | Where |
|---|---|---|---|
| 1 | 150+ cases pass; proptest of the arbiter and the envelope, 10⁶ cases | **Passed** | CI, every commit (cases) and nightly (proptest) |
| 2 | same cases, same bytes on x86_64, a real ESP32, Cortex-M33 and RISC-V | **Passed in CI; ESP32 provisional** | CI: x86_64, aarch64, QEMU `mps2-an505` and `virt`; the ESP32 leg needs the bench |
| 3 | the tick path does not allocate | **Passed** | CI, every commit, on Linux and in QEMU |
| 4 | envelope + arbiter ≤ 300 ns per output on x86; the cost on a Cortex-M33 measured and published | **x86_64 provisional; Cortex-M open** | CI shared x86_64 runners (3 of 4 runs under 300 ns); no Cortex-M33 board, the ESP32 `CCOUNT` measurement needs the bench |
| 5 | the same trace gives the same bytes in 1,000 runs | **Passed** | CI, every commit, on x86_64 and aarch64 |

## Gate 1 — cases and properties

- **Cases.** `conformance/` holds 200 cases: 152 of the core (envelope 75, quality 26, arbiter 22, expressions 17, alarms 10, trace 2) and 48 of the `action` area. `cargo xtask conformance` passes all 200 on x86_64 and aarch64, and the 152 core cases pass in QEMU on Cortex-M33 and RISC-V (the `conformance` and `qemu` jobs). The 150 of the gate are the core cases.
- **Properties.** The nightly `core-proptest` job runs `arbiter_and_envelope_invariants` (`crates/joker-core/tests/props.rs`) with `PROPTEST_CASES=1000000`: run 40173197650 of 2026-10-25, 1,000,000 cases in 65 s, passed.

## Gate 2 — the same bytes on every target

- The `determinism` job compares, case by case, the status and the FNV-1a digest of the result sequence of x86_64 with those of aarch64, Cortex-M33 (`thumbv8m.main-none-eabihf` on `mps2-an505`) and RISC-V (`riscv32imac-unknown-none-elf` on `virt`), and publishes one SHA-256 per target (AUR-221). Every target gives `sha256:902182864e500a929bf5ac589c80d3f61d9b5fc5283dcdedf229f384bf69c295` for the 152 core cases.
- **ESP32: provisional.** The `xtensa` job builds the core for `xtensa-esp32-none-elf` (non-blocking, document 13 §5), but the conformance battery has not run on the bench ESP32: it needs the board of document 23 (M0 friction report, F-1). Until it runs, the gate holds for the four targets above.

## Gate 3 — no allocation on the tick path

- `cargo xtask conformance` runs every tick of every core case inside a guard that counts allocator calls; one call is an `R7` error of the case (docs/10 R7). The 152 core cases report none.
- `crates/joker-core/tests/noalloc.rs` counts allocations over latch, alarms, derived signals, method expansion, the full envelope, the commit and the trace writer.
- `cargo xtask rt-bench` runs every measured tick inside the same guard; an allocation fails the run.
- On the MCU targets, `cargo xtask qemu` links the conformance firmware with no global allocator and fails if an allocator symbol is linked (`allocator_symbols`), on Cortex-M33 and RISC-V.

## Gate 4 — cost per output

- **x86_64: provisional.** The `output-cost` job (AUR-236) runs `cargo xtask rt-bench --max-output-ns 300` on the shared x86_64 runner on every commit: 64 outputs with range and rate and two writers proposing on every output in every tick. Four runs on 2026-10-25 gave medians of 211.7, 278.5, 286.5 and 336.9 ns per output (p99.9 from 481 to 2,409 ns). The code was the same; the spread is the hardware of the shared runners. Three runs meet the bound and one does not, so the job publishes the figure and warns above 300 ns without blocking, and the gate stays provisional until it runs on a dedicated x86 machine, the condition of document 01 §21.
- On the development machine (aarch64 macOS, release): median 185.5 ns, p99 211.6 ns, p99.9 514.3 ns.
- **Cortex-M: open.** The cost on a Cortex-M33 at 150 MHz needs a board (RP2350); QEMU does not model cycle timing, so its figures would not be costs. The interim measurement the plan names, the ESP32 `CCOUNT` around the commit on the bench, needs the bench ESP32 and has not run. Both stay open until the hardware is available, and are reported as such (ADR 0035, decision 8).

## Gate 5 — 1,000 runs, the same bytes

`cargo xtask determinism --runs 1000` (AUR-221) loads the core suites once, runs them 1,000 times and fails unless every run gives the canonical sequence of the first, byte for byte. It runs in the `conformance` job on x86_64 and aarch64 on every commit, in about 6 s; both give `sha256:902182864e50…`, the digest of their own reports. `crates/joker-core/tests/replay.rs` also checks that a recorded `.jktrace` replays to the same bytes.

## Deviations

- The ESP32 legs of gates 2 and 4 and the Cortex-M33 cost of gate 4 are hardware runs that have not happened; the CI covers Cortex-M33 and RISC-V in QEMU for gates 2, 3 and 5.
- Gate 4 is checked on the median and on shared runners, whose hardware varies between runs (211.7 to 336.9 ns); it needs a dedicated x86 runner to be a blocking bound. The 1.0 target of document 01 §21 is the same 300 ns, measured on the hardware of its condition.
