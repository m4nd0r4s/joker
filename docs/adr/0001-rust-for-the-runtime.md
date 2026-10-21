# ADR 0001 — Rust for the runtime

**Status:** accepted (v1) · **Date:** 2026-09

## Context

The runtime runs on devices with 512 MB to 8 GB, for years without a restart, with a real-time part. It needs simple cross-compilation for aarch64, armv7 and x86_64, and a single binary without heavy system dependencies.

## Decision

`jokerd`, `jk`, the tier N drivers and the ML backends are written in Rust (edition 2024), with `unsafe` confined to the `*-sys` crates and to the host FFI.

## Reasons

- No GC: predictable latency in the cycle domain, and controlled memory.
- Memory safety in a process that brings together drivers, networking and ML.
- The ecosystem already exists where it matters: `tokio`, `wasmtime`, `tokio-modbus`, `async-opcua`, `ethercrab`, `socketcan`, `rumqttc`, `zenoh` and **Burn**.
- Cross-compilation and static binaries with `cargo` and `cross`.

## Consequences

- Rejected alternatives: **C/C++** (productivity and memory safety cost more than what is gained); **Go** (GC on the cycle path, and cgo for half of the drivers); **Zig** (no industrial ecosystem).
- The cost is the learning curve for automation contributors, and that is why apps do not require Rust (ADR 0002).
