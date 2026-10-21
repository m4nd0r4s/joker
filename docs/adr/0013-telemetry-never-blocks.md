# ADR 0013 — Telemetry never blocks control

**Status:** accepted (v1) · **Date:** 2026-09

## Context

A slow disk, a network that is down or an overloaded MQTT broker must not delay a tick.

## Decision

Services consume the core only through bounded lock-free queues. Full queues discard according to the declared policy, and each discard is counted and exposed as a `joker.*` signal. Store-and-forward reads from the historian instead of keeping queues in memory.

## Reasons

- Control does not depend on anything outside the device.
- Drops are never silent.

## Consequences

- Under extreme conditions, telemetry samples are dropped. Alarms and the decision log have the highest priority and their own fsync.
