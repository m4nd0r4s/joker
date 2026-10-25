# ADR 0080 — How `joker-spec-tests` compiles the references

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Task 0a (document 13, §4; document 22, §8) asks a `joker-spec-tests` crate to compile the Rust blocks of the references as doctests, to run `tsc` on the TypeScript blocks, to validate the CDDL with a complete validator against the examples, to validate every manifest block against the JSON schema generated from the types, to validate the OpenAPI, and to check the counts that the prose repeats. ADR 0035 adds two rules: a code with `since` later than 1.0 is reserved, and an operation with `x-joker-since` later than 1.0 answers 501.

Writing the crate showed that the task, read literally, cannot be done, and that some parts need a decision:

- The Rust blocks of the references are **signature listings**: `fn` without a body, `{ /* … */ }`, one-line `impl` blocks. No compiler accepts them, and turning them into compilable code would make them worse as a reference. The example blocks compile only against a complete SDK, which does not exist yet (AUR-284).
- The TypeScript SDK is 1.2 (ADR 0035, decision 3): there is no package to type-check the example blocks against.
- The only complete CDDL validator is the reference tool of RFC 8610, the Ruby `cddl` gem; the Rust `cddl` crate is incomplete and would bring about 130 crates into `Cargo.lock`, each to be vetted (ADR 0055). The examples of the references do not say which rule they illustrate.
- A block of a reference is often a fragment: a manifest block without `[project]`, an overlay with `{ unset = true }`, a TypeScript member of an object.
- Many divergences exist today. The rule of Phases 0–1 is that a divergence opens a triage issue (document 26, §9), not that CI stays red until everything is implemented.

## Decision

1. **Rust blocks.** Each Rust block of `docs/ref/` is read for what it declares: every public type, trait, function, constant and module, and every public method or associated constant of an `impl` or a `trait`, except what an `unstable-*` feature gates (reserved). A block with `#[joker::…]` or `fn main` is an example, and what it uses (`joker::<item>`, `joker_client::<item>`, `joker_hal::<item>`) is checked instead. The names are looked up in the crates that implement the reference, parsed with `syn`; the crates of each reference are a table of the test. Compiling the example blocks as doctests comes when the SDK they use is complete; until then, resolution of names is the check.
2. **TypeScript.** Every `ts` block of the documentation must parse. The declaration blocks of ref/05 (no `import`, starting with `export`) are type-checked together under `--strict` as the declarations of the SDK. Type-checking the example blocks against them comes with the TypeScript SDK (1.2).
3. **CDDL.** The validator is the `cddl` gem (RFC 8610), with `cbor-diag` for the diagnostic notation, installed in CI at pinned versions. Each file of `spec/cddl/` must generate instances that validate back against it: an undefined, duplicated or unsatisfiable rule fails. Each example block (`cbor-diag` in the references, `json` in ref/10) is mapped in `crates/joker-spec-tests/known.toml` to its file and rule, by the fingerprint of its text and its section; an unmapped block fails. The tool stays outside the Cargo graph.
4. **Manifest blocks.** Every TOML block of the documentation whose root keys are all manifest root keys is validated against the schema that `joker-manifest` generates (`schema::json_schema`). Fragments are allowed: the `required` lists are dropped, and the merge directives of ref/01 §7.2 are applied as the effective manifest would see them. Alternative forms of a key are `anyOf` in the generated schema, not `oneOf`: a quantity literal is also a valid expression, and the forms are not exclusive.
5. **OpenAPI.** The document must be OpenAPI 3.1, every `$ref` must resolve, operation ids are unique, path parameters are declared, every path is under `/v1` except `/metrics` (ADR 0071), every operation has `x-joker-since`, every schema compiles as JSON Schema 2020-12 and every example fits its schema. An operation with `x-joker-since` later than 1.0 has a 501 response with `NotImplemented` (`J-E1017`). Every operation of a ref/09 table exists in the OpenAPI, and every operation of the OpenAPI is named in ref/09.
6. **Counts and codes.** The counts that the prose repeats are checked against their sources: the Joker Link messages (ref/07 §1, document 26 §5) against `link-body` of the CDDL and the 1.0 rows of ref/07 §6.1; the references and the API operations of document 26 §5 against `docs/ref/` and the OpenAPI (within 15 %, the prose says "~"). A code with `since` later than 1.0 has `status = "reserved"`, and a reserved code has `since`.
7. **Known divergences.** `known.toml` lists, per check and scope, the divergences found and the triage issue that tracks each scope, and the blocks that are deliberate excerpts with the reason. A divergence that is not listed fails CI, and so does a listed one that is gone: the list only shrinks, and only when the reference or the code is fixed.
8. **CI.** The `spec-tests` job is blocking, runs on every commit, and sets `JOKER_SPEC_TOOLS=required`, so a missing tool fails. Without it (a developer machine), the checks that need `node`, `typescript`, `cddl` or `diag2cbor.rb` are skipped with a note.

## Consequences

- `crates/joker-spec-tests` holds the checks as integration tests; it has no library code and only development dependencies already in `Cargo.lock`.
- The CDDL tool found two errors in the files: `agent.cddl` redefined the prelude types `time` and `bytes`, and `joker-link.cddl` the prelude type `time`. They are renamed `timestamp`, `binary` and `time-msg`; the wire format does not change, and ref/10 §2.1 follows. The generated manifest schema used `oneOf` for the alternative forms of a key and rejected valid manifests; it uses `anyOf`. Both are recorded in `docs/errata/spec-tests-2026-10.md`.
- The divergences found when the job was written are in `known.toml`, with AUR-284 (Rust SDK), AUR-241 (test harness), AUR-315 (`joker-micro`), AUR-387 (TypeScript SDK), and the triage issues AUR-429 (ref/01 §12 model), AUR-430 (`joker-hal` items of ref/06 §3) and AUR-431 (CDDL examples).
- Changing a check of this ADR, or the tools it uses, is a change of this ADR.
