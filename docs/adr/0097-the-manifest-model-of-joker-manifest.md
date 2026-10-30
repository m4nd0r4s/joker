# ADR 0097 — The manifest model of `joker-manifest`: schema as data and a typed model

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Ref/01 §12 sketched the manifest as one tree of `serde` types (`Manifest`, `Envelope`, `Safe`, `Bound`, `Instance`, `ImplementedBy`, `Reserved<T>`, `Ext`, value newtypes such as `Quantity`, `Rate`, `Expr`, `Selector`, `ActuationBudget`), and said the JSON schema is generated from them. `joker-spec-tests` (ADR 0080) found that eleven of those names do not exist in `joker-manifest` (AUR-429).

The crate was built another way, for reasons the sketch did not have:

- The whole 1.0 schema, with the phase of each key (1.0, 1.1, reserved) and its target (Linux, Joker Micro), is one table of data (`schema::Key`, `schema::Ty`). One table drives the generic validation of every section, `J0290` for reserved keys, `J0145`/`J0720` for the targets and the generated JSON schema, so the four cannot disagree. With `serde` types, the phase and target of a key would live in attributes spread over many types, and `Reserved<T>` and `Ext` would be wrappers on every field.
- The typed model (`model::Manifest`) covers the sections the runtime consumes; a 1.0 section without a decoder yet is kept validated in `Manifest::pending`, never rejected nor trimmed.
- Values keep their span everywhere (`Spanned<T>`, `value::Node`), for the diagnostics of ref/01 §9.
- Expressions, bounds and quantities stay text in the model and are compiled or converted once the registry of paths exists, so there is no `Expr`, `Bound` or `Quantity` newtype.

Ref/01 §12 is a sketch, not a contract: the contract of ref/01 is the TOML grammar, the codes and the published JSON schema, none of which change.

## Decision

1. Ref/01 §12 describes the three layers of the crate (the spanned untyped tree, the schema as data, the typed model with `pending`) and its block names the real types, with `check` as the entry point and `schema::json_schema` as the source of the JSON schema.
2. The introduction of ref/01 says that the JSON schema is generated from the schema as data.
3. The crate does not change.

## Consequences

- The `[divergence.rust."ref/01 §12"]` scope of `crates/joker-spec-tests/known.toml` is gone; a new type named in §12 must exist in the crate.
- The glossary entry on the type metasyntax of ref/01 §2.2 points at `schema::Ty`.
