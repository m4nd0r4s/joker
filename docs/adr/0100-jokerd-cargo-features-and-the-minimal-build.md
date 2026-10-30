# ADR 0100 — The Cargo features of `jokerd`, its Wasm backends and the `minimal` build

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01 §21 sets two size targets for `jokerd`: the `minimal` build (`sim`, `sysfs`, Modbus, MQTT) at 20 MB and the full build at 30 MB, stripped, aarch64. Ref/06 §3.8 lists the Cargo features that make the builds. `jokerd` had no features (AUR-432): every driver and the whole Wasm host were always in, so the `minimal` build did not exist, and the full build measured 26.0 MB stripped on aarch64-apple-darwin. `cargo bloat` showed where it went:

- the workspace `wasmtime` enabled `all-arch`, so an aarch64 `jokerd` carried the x86-64 Cranelift backend (`cranelift_assembler_x64` 1.5 MB and part of `cranelift_codegen` 5.0 MB) and Pulley. Only a host that compiles AOT for another target needs them: `jk build --target` (ADR 0052), never the node;
- the drivers weigh little (about 0.2 MB for `gpio`, `i2c` and `revpi` together);
- the workspace had no release profile for shipping: thin local LTO and 16 codegen units.

ADR 0099 decided that the 1.0 registry by features is Cargo features in `jokerd`, without `DriverEntry` or `build.rs`.

## Decision

1. **Driver features.** `jokerd` has `drv-sysfs`, `drv-modbus`, `drv-gpio`, `drv-i2c` and `drv-revpi`. Each makes its driver crate a dependency and turns on the driver's catalog entry and its arm of the device dispatch. `default` has all of them; `minimal` has `drv-sysfs` and `drv-modbus`. `sim` is always built in, without a feature. `drv-mqtt` and `drv-joker-link` join `default` and `minimal` when their drivers ship. Every combination of features builds without warnings.
2. **A driver outside the build.** The manifest context of `jokerd` lists the tier N drivers the build leaves out (`Context::absent_drivers`); a device of one is `J0850` ("tier N driver not compiled into the node's runtime"), not `J0170` (unknown driver).
3. **Wasm backends.** The workspace `wasmtime` has no `all-arch`. `joker-host` adds the `pulley` feature on 32-bit Arm, whose engine compiles for Pulley (`compile_target`), and has an `all-arch` feature that `jk` turns on to precompile for other nodes. The engine identity of ADR 0052 does not change with the backends compiled in (checked: the same identity with and without `all-arch`).
4. **The `dist` profile.** The shipped `jokerd` is built with the Cargo profile `dist`: `release` with fat LTO and one codegen unit. The size gate (`cargo xtask size`) measures with `cargo bloat --profile dist`. Other release builds (benchmarks, conformance) keep `release`, so they do not pay the link time.
5. **The gate.** `xtask/size-limits.toml` lists the `minimal` build (`--no-default-features --features minimal`) with a limit of 20 MB, next to the full build at 30 MB.

## Consequences

- Measured with `dist`, stripped: on aarch64-unknown-linux-gnu (the CI gate) full 18.36 MB and `minimal` 18.22 MB; on aarch64-apple-darwin full 17.9 MB and `minimal` 17.8 MB, where the full build was 26.0 MB with `release` and `all-arch`. The `minimal` build is close to the full one because the Wasm host is in both: apps are Wasm components in every build (ADR 0002).
- `jokerd`'s tests use the driver crates directly, so the crates are also development dependencies.
- Document 01 §21 and ref/06 §3.8 follow.
