# ADR 0029 — English is the project language

**Status:** accepted (v1) · **Date:** 2026-10

## Context

The specification, the ADRs, the commit messages and the working rules were written in Portuguese, while code identifiers, comments and error messages were already English (AGENTS.md rule 11). The repository is public, the Joker targets an international industrial audience, and contributors, issue tracking and code review all happen in English. Two languages in one corpus also double the vocabulary that has to stay consistent between the references and the code (ADR 0023).

## Decision

1. The whole project is written in English: code, docstrings, error and diagnostic messages, commit messages, pull requests, issues, `docs/` (including ADRs, references, the learning track and examples), `spec/` and `wit/`.
2. Code carries no comments. Public and internal items are documented with docstrings, in English. Configuration files (Cargo manifests, CI workflows, tool configuration) carry no comments either.
3. The existing Portuguese corpus is translated without changing its meaning. Document and section numbers are kept, so a remission such as `docs/16 §3` stays valid; file names get English slugs.
4. A single glossary (`docs/GLOSSARY.md`) fixes the English term for every domain word, and every translation follows it.
5. Until the translation milestone closes, a Portuguese document remains normative; its English translation replaces it in the same commit set, never alongside it.

## Consequences

- AGENTS.md rule 11 is replaced by this ADR.
- The translation is planned as its own milestone, before or in parallel with the first vertical slice (M0), and ends with a CI check that flags leftover Portuguese and broken remissions.
- Codes and their messages in `spec/codes.toml` become English, which is what `jk explain` and the API return.
