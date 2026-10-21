# ADR 0016 — One process, not microservices

**Status:** accepted (v1) · **Amended by ADR 0019 (ML in 1.1)**: model processes (`isolation = "process"`) are in 1.1 · **Date:** 2026-09

## Context

Platforms such as EdgeX split the runtime into several services in containers. That gives isolation, but it costs memory, latency and operational complexity on a small device.

## Decision

One `jokerd` process with threads per domain; isolation through the Wasm sandbox (apps and W drivers) and through auxiliary processes only where the code is not trusted or can hang (X drivers, models with `isolation = "process"`).

## Reasons

- A read goes from the driver to the app without IPC.
- A single artifact to update, monitor and debug.

## Consequences

- A memory bug in an N driver brings the process down. Mitigated by Rust without `unsafe` outside `*-sys`, by the systemd supervisor, by the hardware watchdog and by the device fallback.
