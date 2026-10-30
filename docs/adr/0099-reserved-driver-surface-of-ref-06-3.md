# ADR 0099 — The reserved driver surface of ref/06 §3, `AdcRead`, and the 1.0 registry by features

**Status:** accepted (v1) · **Date:** 2026-10

## Context

`joker-spec-tests` (ADR 0080) found items of ref/06 §3 that neither `joker-hal` nor the drivers have (AUR-430): `Scanner` and `Scanner::scan`, `Cx::locked`, `Cx::spawn_blocking`, `AdcRead`, `CanFrames`, the `MODBUS_TCP` entry and `joker_hal::DriverEntry` / `driver_entry!` of §3.8. ADR 0047 already listed most of them as M0 gaps "with their surface reserved as written", but the blocks did not mark them, so the check could not tell a reserved item from a missing one. ADR 0028 makes stable what is implemented; the rest is reserved.

Per item:

- **`Scanner`.** No catalog driver of 1.0 has the `scan` capability; ADR 0095 answers `jk discover` with a SunSpec walk in `jokerd` and leaves network scans for a driver that scans.
- **`Cx::locked`.** It hands the `[discovered.<d>]` of the lock to the driver, which ADR 0077 §8 leaves open.
- **`Cx::spawn_blocking`.** No tier N driver of 1.0 wraps a blocking library.
- **`AdcRead`.** The `adc` driver of Joker Micro is 1.0 (document 26 §5) and the trait has no dependency: it costs nothing to have now.
- **`CanFrames`.** CAN is 1.1 (document 26 §5), and the trait needs `embedded-can`.
- **`DriverEntry`, `driver_entry!`, `MODBUS_TCP`.** A type-erased entry (`plan` over CBOR, `open` returning `Box<dyn ErasedDriver>`) is what a host needs to run drivers it does not know at compile time: the tier W and X hosts, 1.2. `jokerd` 1.0 knows its drivers at compile time; each needs its own link setup (serial port, TCP endpoint, GPIO chip, I²C bus, piControl paths), so a uniform `open` would only move that code behind a pointer. The Cargo features of §3.8 are what the minimal and full builds of document 01 §21 need (AUR-432), and they do not need the entry nor a `build.rs`.

## Decision

1. `AdcRead` is in `joker-hal` (module `mcu`, `no_std`), with `read` returning `impl Future` as the other async traits of the crate do.
2. `Scanner`, `Cx::locked`, `Cx::spawn_blocking`, `CanFrames`, `DriverEntry`, `driver_entry!` and the `<DRIVER>` constants are **reserved**. Their blocks in ref/06 §3 carry `#[cfg(feature = "unstable-<name>")]`, the marker of reserved Rust items (ADR 0080, decision 1), and ref/06 §3.4 lists the feature names and when each item arrives. `joker-hal` 1.0 does not define those features.
3. In 1.0, the registry by features of ref/06 §3.8 is: each driver crate is an optional dependency of `jokerd` behind its `drv-*` feature, and the feature gates the driver's catalog entry (what `jk check --node` sees) and its arm of the device dispatch. A device whose driver is not in the build is `J0850`. There is no `build.rs`.
4. The reader of `joker-spec-tests` also skips what an item gated on an `unstable-*` feature uses (`joker_hal::DriverEntry` inside the gated `MODBUS_TCP`): what a reserved item uses is reserved with it.

## Consequences

- The `[divergence.rust."ref/06 §3"]` scope of `crates/joker-spec-tests/known.toml` is gone.
- ADR 0047's list of M0 gaps loses `AdcRead` and the `Tcp`, `I2c` and `Gpio` links, which exist.
- AUR-432 implements decision 3.
