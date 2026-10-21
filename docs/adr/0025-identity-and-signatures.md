# ADR 0025 — P-256 identity on nodes, Ed25519 in the organization, COSE for everything that is signed

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Document 01 §17 only mentioned Ed25519. But a node's identity should live in a TPM 2.0 or in a secure element, and those support P-256, not Ed25519. At the same time, artifacts, generations and MCU blobs need signatures verifiable on a microcontroller, and the Deck's desired state needs a form that does not depend on the transport.

## Decision

1. **Node identity:** X.509 **P-256** certificates issued by the organization's CA, with the key in a TPM or secure element when one exists. Used in the mTLS of the API and of the agent, and in signatures made by the node itself (`ES256`).
2. **Organization keys** (runtime, config, apps, drivers, profiles, boards, classes, firmware, secrets, deck): **Ed25519**, outside the nodes.
3. **Everything that is signed is COSE_Sign1 over deterministic CBOR** (desired state, trust bundle, generation blob, OTA manifest, reports), even when it travels over HTTPS. OCI artifacts also carry signatures compatible with cosign or notation.
4. **Trust:** a bundle signed by an offline root says which key signs which role (a reduced TUF). The manifest's `[security.trust]` only **restricts** that bundle, never adds keys.
5. The node keeps controlling with the certificate expired, revoked or missing: identity is for talking to the outside, not for deciding about outputs (document 20, principle 5).

The detail is in ref/02 §2.5, ref/10 §2.4 and ref/08 §2.13.

## Consequences

- Two curves in the code base (P-256 and Ed25519); both have mature `no_std` implementations.
- Document 01 §17 and 03 §13 are updated.
