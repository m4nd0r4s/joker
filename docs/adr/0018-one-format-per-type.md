# ADR 0018 — One format per kind of thing: TOML, `.jkc` and code

**Status:** accepted (v1) · **Date:** 2026-09

## Context

The project has accumulated several file types (document 15). Equipment classes (ADR 0017) started in TOML and exposed the problem: expressions inside strings, methods in inline tables, bad errors. There were three paths: TOML/YAML for everything, a DSL, or code (Rust, TS, Python) as the source.

## Decision

Each file has the format of the **kind of thing** it describes:

| Kind | Format | Examples |
|---|---|---|
| configuration (data that `jk check` analyzes, merged by overlays, reviewed in a PR) | **TOML** | manifest, overlays, lock, profiles, boards, simulation models, scenarios, conformance |
| contracts (interfaces shared between languages and tools) | **`.jkc`** (document 17) | equipment classes |
| behavior | **code** (Rust tier 1; TS and Python tier 2), always in Wasm | apps, active implementations, extension traits, local classes |

YAML is not used anywhere. Configuration in code is rejected: what Joker must analyze must be data. To generate configurations at scale, any language that **emits** TOML is used.

## Reasons

- Configuration must be analyzable, mergeable and readable by those who do not program.
- A contract must be neutral (several languages, profiles, CLI, HMI, OPC UA) and have syntax for methods, actions and queries; `.jkc` reuses the expression parser that already exists.
- Behavior needs total freedom, and the Wasm sandbox makes it safe.

## Consequences

- A parser, a formatter and an LSP for `.jkc` (`jk class fmt|check`), and an LSP for the TOML files with support for the embedded expressions.
- Local classes in Rust crates remain valid for use inside apps; publishing them to the registry requires writing the `.jkc` (no automatic extraction in v1).
- Required validation in Phase 1: write the `std/` classes in `.jkc`; if the DX is not clearly better, the decision is revised in a new ADR.
