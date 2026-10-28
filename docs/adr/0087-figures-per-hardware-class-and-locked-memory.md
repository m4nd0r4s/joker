# ADR 0087 — The figures of document 01 §21 per hardware class, and memory as `jokerd` locks it

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01, §21, sets "envelope + arbiter per output ≤ 300 ns" (no atomic groups) and "idle RSS without ML ≤ 60 MB" (1,000 signals, 3 apps, historian cache of 8 MB, the resident `.text` counts) without naming the hardware, while its jitter rows name the CM4/CM5. The M0 bench is a Raspberry Pi 3 B+ (Cortex-A53 at 1.4 GHz, Raspbian 9 32-bit, `jokerd` built for `arm-unknown-linux-musleabihf`, ARMv6 code), and on it the M0 friction report (2026-10-27 update) measured 9.16 µs per output at the median and 93 MB of RSS 15 s after boot with one Wasm app, two devices and the historian. AUR-436 profiled both:

- **Cost per output.** The commit matched every output against every staged proposal and every proposal against the later ones: quadratic in the size of the group, so the cost per output grew with the group (on an M4 Pro, 83 ns at 8 outputs, 173 ns at 64, 1.56 µs at 1,000). With that and two smaller fixes gone, the median is 43 ns at 64 outputs and 44 ns at 1,000 on the M4 Pro's performance cores, and 126 ns and 179 ns on its efficiency cores (740 ns and 7.7 µs before). What is left is linear work per output: two arbiter intakes, the envelope with range and rate, the decision tracks and a seqlock write with two barriers, in ARMv6 code without a hardware divide or 64-bit integer to float conversion. On the in-order Cortex-A53 that is estimated at 1–2 µs per output, not 300 ns; the CM4 and CM5 (out-of-order Cortex-A72/A76, aarch64 code) are the class the 300 ns were set for.
- **RSS.** `jokerd` locks its memory with `mlockall(MCL_CURRENT | MCL_FUTURE)` (document 09, attack 5), so on Linux every mapped page is resident: the whole binary, every thread stack at its full size and every pre-allocated buffer, touched or not. Three things were far above what the design assumes: the continuous trace ring kept each tick in a buffer sized for the largest possible tick (~9.3 KB instead of a few hundred bytes), so the RSS grew by ~0.5 MB/s with one 20 ms group, towards ~330 MB at the 10 minute retention; the historian's telemetry queue took 18 MB at boot; and every service thread had the standard library's 2 MiB stack. With those fixed, `jokerd` on the bench project goes from 44 MB at 15 s and 123 MB at 3 minutes to 29 MB and 30 MB on macOS, where nothing is locked.

## Decision

1. **Reference class.** The §21 figures that name no board are figures of the reference class of 1.0: CM4 and CM5 (Cortex-A72/A76) with an aarch64 build, the class the jitter rows already name. "Envelope + arbiter per output ≤ 300 ns" and "Idle RSS without ML ≤ 60 MB" are met there.
2. **Other boards publish and do not regress.** On other boards (the Pi 3 B+ of the bench, 32-bit builds, Joker Micro's MCUs), `cargo xtask rt-bench` and the RSS are measured and published in the errata of the phase, with the board, the build target and the kernel, and a change that makes them worse by more than 5% at p99.9 is a regression (AGENTS.md §5); they carry no absolute bound. The Pi 3 B+ is the M0 bench, not a target of the 1.0 figures.
3. **The cost per output does not depend on the size of the group.** `cargo xtask rt-bench --max-scaling 3` blocks every commit (document 13, §5): the median cost per output of a group of 1,024 outputs is at most three times that of 64.
4. **RSS is measured the way the node runs.** With the daemon's `mlockall`, 15 s after boot and again after the retention of the continuous trace, under the condition of the row. What `jokerd` locks is therefore declared: every thread it spawns besides the `rt/<group>` threads and the tokio runtimes has an explicit stack (512 KiB for the service threads), every queue allocated at boot has a size in records, and the continuous ring holds each tick at its encoded size (the estimate of ADR 0048).

## Consequences

- Document 01, §21, names the reference class in the condition of both rows.
- The Pi 3 B+ figures of the M0 friction report are re-measured with this change and published as the bench's own; nothing in 1.0 is gated on them.
- A node that never compiles Wasm does not need Cranelift's backends for every architecture (`all-arch`, 4.5 MB of the 27.7 MB macOS `jokerd`): under `mlockall` the whole binary is resident, so the build features of `jokerd` (AUR-432) count towards the RSS row.
