//! Task 0a of Phase 0 (docs/13 §4, docs/22 §8): the references are checked
//! against the code and against the machine-readable files of `spec/`,
//! so that CI fails when a reference and the code diverge (ADR 0023).
//!
//! The checks are the integration tests of this crate (ADR 0080):
//!
//! - `rust_blocks`: the items that the Rust blocks of the references
//!   declare or use exist in the crates that implement them;
//! - `typescript`: every TypeScript block parses, and the SDK declarations
//!   of ref/05 type-check under `tsc --strict`;
//! - `cddl`: every CDDL file is complete, and every example of the
//!   references fits its rule, with the reference CDDL tool;
//! - `manifest_schema`: every manifest block of the documentation fits the
//!   JSON schema that `joker-manifest` generates from its types;
//! - `openapi`: the OpenAPI is valid, every operation reserved after 1.0
//!   answers 501, and ref/09 lists the same operations;
//! - `counts`: the counts the prose repeats match their sources, and every
//!   code after 1.0 is reserved (ADR 0035).
//!
//! Known divergences live in `known.toml`, each scope with the triage
//! issue that tracks it; a new divergence and a stale entry both fail.
