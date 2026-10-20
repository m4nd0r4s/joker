# ADR 0022 — Versioning policy: themed minors, full compatibility in 1.x, LTS at the end of the series

**Status:** accepted (v1 → v2 plan) · **Date:** 2026-09

## Context

After 1.0 there is more work than fits in one version (document 21): ML, Structured Text, protocols, security, Studio, Wasm on MCUs, the Deck at scale, redundancy. Industrial customers put a node in the field and do not want to touch it again for years. It is necessary to decide what may change, when, and for how long a version is supported.

## Decision

1. **Golden rule of 1.x:** a 1.0 installation that works keeps working, the same way, in every 1.x. No minor breaks apps (`joker:runtime@1.y`), manifests, locks, `std/` classes, Joker Link, the agent protocol, nor changes an existing conformance result (document 16). The only exception is a security fix, with its own ADR and automatic migration.
2. **Themed minors**, one every ~3–4 months: one theme per version, at most three headline features, exit gates as in the v1 phases. What does not pass the gates moves to the next version.
3. **New drivers and classes may ship in any minor**, outside the theme, because they are extension points and do not touch the core.
4. **Support:** each minor receives fixes until six months after the next one; 1.0 is supported for 12 months; the **last minor of series 1 is LTS**, with 3 years of security fixes and critical defect fixes.
5. **The Deck and the Hub have their own versions.** Compatibility with nodes is through the agent protocol version, and a Deck supports nodes of the last three minors and of the LTS.
6. **A major (v2)** only happens when a necessary change does not fit the golden rule (per-node qualified paths, distributed arbiter, new WIT world). It always has `jk migrate`, 2.0 reads the last 1.x, and world 1 apps run through an adapter for the whole of series 2.

## Reasons

- The trust of industrial customers comes from predictability, not from speed.
- Themes and the feature limit avoid bloated and late minors.
- The separation between core and extension points makes it possible to respond to pilots without breaking the cadence.
- The LTS gives an "install and forget" version, without forcing every minor to be maintained.

## Consequences

- `jk check` and the CI have a conformance set per version, which never loses cases.
- Each headline feature needs an ADR before entering a minor.
- Document 21 is updated at the end of each version.
- Series 2 must keep the world 1 adapters, which is an accepted maintenance cost.
