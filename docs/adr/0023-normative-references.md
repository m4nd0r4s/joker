# ADR 0023 — Interfaces are closed in normative references (`docs/ref/`)

**Status:** accepted (v1) · **Date:** 2026-09

## Context

The per-area documents (03 manifest, 04 SDK, 05 drivers, 19 Joker Micro, 20 Deck) explain the design with examples, but they were not exhaustive: there were keys used in the examples without a definition, messages without fields, errors without a code and two documents saying different things about the same field. Implementing from them would force detail to be invented during implementation, and each team would invent its own.

## Decision

1. Each interface has an **exhaustive reference** in `docs/ref/`, with the common representation fixed in `ref/00-conventions.md` (document 22).
2. **Precedence:** `ref/00` has the level of document 16. `ref/01`–`ref/10` sit **above** the per-area documents, which become the explanation and refer to the reference for the detail. The full order is in `docs/README.md`.
3. The machine-readable files (`wit/`, `spec/cddl/`, `spec/openapi/`, `spec/c/`) are part of the reference: if a file and the text diverge, it is a bug to fix in both, and the Phase 0 CI (task 0a, document 22, §8) prevents that from happening silently.
4. Changing an interface is changing the reference and the machine-readable file in the same set of commits, with an ADR if it touches a public contract (ADR 0022).

## Reasons

- Implementation only works on specs; doubts are resolved in the reference, not in the code.
- The machine-readable files generate code (WIT bindings, OpenAPI clients, CBOR validation) and conformance tests.

## Consequences

- The per-area documents shrink in detail and gain cross-references.
- The references are large (about 1.2 MB of text) and need an owner per front (document 13).
- Task 0a of Phase 0 compiles and validates the references before any feature.
