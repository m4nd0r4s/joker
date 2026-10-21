# ADR 0011 — Arbiter with priorities and leases

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Several sources want to write to the same output: operator, software protection, supervisor, controller, optimizer, ML. Without an explicit rule, whoever wrote last wins, and that is the "the operator changed it, the model undid it" problem.

## Decision

16 levels (1 is the highest) with named bands; each writer occupies a declared level; each write has a lease; the active lease with the highest priority wins; with none active, the output goes to `safe`; the manual levels have a mandatory TTL.

## Reasons

- It is the BACnet *priority array* model, tested over decades of building management, with leases added to cover writers that die.
- The lease is each writer's dead man's switch.

## Consequences

- An output with several writers forces each one to declare a level and a lease (`J0211`). With a single writer there is a default (`control:12`, a lease of 3 periods), so that the minimal manifest stays small.
- With equipment that also has priorities (BACnet), the level on the equipment is declared per device.
