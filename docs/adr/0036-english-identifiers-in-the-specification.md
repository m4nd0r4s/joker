# ADR 0036 — English identifiers in the specification

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0029 made the whole project English and asked for a translation that keeps the meaning. The translation followed the glossary rule that identifiers are never translated (`docs/GLOSSARY.md` §1.4), so every Portuguese identifier of the corpus survived it: the app `luz` of the learning track and its cycle `pisca`, the instances `botao`, `toque` and `sinal`, the alarm `cpu_quente`, the cohorts `canario` and `estavel`, the key `segredos`, file names such as `incidente.jktrace` and `keys/minha.pub`, and SDK test names such as `qualidade_ma_nao_escreve`. Most of them are illustrative: they name things in examples, and nothing reads them. Two are not. They are literals that a reference defines and that an implementation has to read:

- `"$valor"` (ref/03 §2.2.10): in the `apply` steps of a `[config]` key of an equipment profile, the placeholder for the value chosen for that key.
- `n_largos` (ref/04 §3.2): the number of wide-section entries in the `image-len` formula of the cycle ABI.

A Portuguese identifier in an English specification breaks the rule that code is English (AGENTS.md rule 11, ADR 0029), and an example in which the names do not mean anything to the reader teaches less. Renaming the literals changes a contract (ADR 0022, ADR 0023), so it needs a decision.

## Decision

1. **The illustrative identifiers are renamed to English**, consistently, across `docs/`, `spec/` and `wit/`. The map is Appendix B of the glossary (for example `luz` → `light`, `pisca` → `blink`, `botao` → `button`, `toque` → `touch`, `bancada` → `bench`, `canario` → `canary`, `estavel` → `stable`). It is case-sensitive and applies to whole identifiers, and it covers the file names, directories, node and site names, keys and cohorts of the examples.
2. **Two normative literals change:** `"$valor"` becomes **`"$value"`** in ref/03 §2.2.10, and `n_largos` becomes **`n_wide`** in ref/04 §3.2. No implementation reads either of them yet, and v1 has not shipped, so the old spellings are not kept as aliases.
3. **Proper nouns stay:** place names used as site and node names (`lisboa-01`, `revpi-porto`), product and vendor names, and identifiers that belong to third parties (`jefe` of Hubris, the `modbus-slave` driver name).
4. **New examples use English names** from the start. The glossary rule of §1.4 still holds: a translator does not rename an identifier; renaming one is a decision like this one.

## Consequences

- The documents, the learning track, the examples and the proposal change in the same commit set as this ADR. A reader who meets an old name (in git history, an issue or a branch) finds the new one in Appendix B.
- `spec/` and `wit/` contain neither normative literal, so no machine-readable file changes for decision 2; ref/03 and ref/04 are the only references affected.
- The profile loader (`joker-manifest`, ref/03) recognizes `"$value"` in the `apply` steps of a `[config]` key. A `[config]` key has no `args`, so `"$value"` cannot collide with the `"$<argument>"` of an action step (§2.2.12).
- The cycle ABI is unchanged: `n_wide` names the same count that `n_largos` named.
- Branches that still use the old names (the ESP32 bench firmware, conformance cases) follow the map when they are next rebased on the English specification.
