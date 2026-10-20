# ADR 0010 — Declarative TOML manifest and `joker.lock`

**Status:** accepted (v1) · **Date:** 2026-09

## Context

The configuration of a site is reviewed by automation and software people, lives in Git and is multiplied across hundreds of sites. Part of it is not declarable (discovered maps, firmware, hashes).

## Decision

`joker.toml` in TOML, with unit-bearing quantities in strings, a published JSON Schema and overlays (`extends`). Everything that is resolved or discovered goes into `joker.lock`, versioned, with drift detection.

## Reasons

- TOML is well known, reviewable in a PR and well supported by editors.
- The lock gives reproducibility (the same manifest + lock pair produces the same system) and catches firmware changes.

## Consequences

- Rejected alternatives: **YAML** (type ambiguities); **a custom DSL** (one more language to learn; it is reserved for expressions); **configuration in code** (less reviewable by those who do not program).
