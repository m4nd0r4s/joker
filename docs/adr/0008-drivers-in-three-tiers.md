# ADR 0008 — Drivers in three tiers

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Drivers are the largest investment. Some are critical and universal (Modbus), others are third-party niches, and others still depend on vendors' binary SDKs.

## Decision

- **N** (native, Rust, compiled in): base protocols.
- **W** (Wasm, with provided transports): third-party protocols.
- **X** (external process, JDP): binary SDKs and untrusted code.

## Reasons

- The essentials are fast and safe; the ecosystem grows without touching `jokerd`; blobs stay isolated.
- No dynamic Rust plugins (unstable ABI).

## Consequences

- Adding an N driver takes a runtime build (`jk runtime build --features`).
- JDP is a protocol to maintain and to version.
