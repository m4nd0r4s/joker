# ADR 0094 — Seeded CDDL generation, and signed 64-bit integers in the CDDL files

**Status:** accepted (v1) · **Date:** 2026-10 · **Amends:** ADR 0080, decision 3

## Context

ADR 0080 (decision 3) asks every file of `spec/cddl/` to generate instances that validate back against it, with the reference tool of RFC 8610. The check ran `cddl <file> generate 5`, so each run drew new random instances. It failed on a pull request that did not touch `spec/` (AUR-437): about one run in fifty failed on `jdp.cddl`.

The cause is in `jdp.cddl`, not in the tool. Two rules wrote a signed 64-bit integer as `int .size 8`: case 2 of `raw-value` and key 3 (`ts-src`) of `raw-sample`. RFC 8610 §3.8.1 defines `.size` on byte strings, text strings and unsigned integers only. On `int`, a choice of `uint` and `nint`, the control has no meaning, and the generator stops with "Don't know yet how to generate". The failure only showed when the random walk reached one of those two rules.

A random walk that changes on every run also makes CI fail on changes that have nothing to do with the CDDL, and lets a real error pass for weeks.

## Decision

1. **Signed 64-bit integers.** The CDDL files write a signed 64-bit integer (WIT `s64`) as the range `int64 = -9223372036854775808..9223372036854775807`, defined in `jdp.cddl`. `.size` is used only where RFC 8610 defines it. The values and the encoding do not change: a CBOR integer in that range.
2. **Seeded generation.** The check seeds the generator of the tool. For each file and each seed of a fixed list (1 to 8), one Ruby process loads the `cddl` gem, seeds Ruby's random generator, generates five instances, encodes each one as CBOR, decodes it and validates it against the file. The same commit checks the same instances on every run, and a failure names the file, the seed and the instance.
3. **One process per seed.** A process does not carry the state of the parser from one seed to the next, so a seed reproduces alone: `ruby -e '<script>' spec/cddl/<file> <seed> 5`.
4. **Wildcard keys.** The generator of the tool draws the keys of a wildcard member (`* uint => any`) at random, and can draw a key the same map spells out (`? 9 => bstr .size 32`), which gives an instance that does not fit the file. The check makes a wildcard member skip the keys the map spells out; nothing else of the generator changes.
5. The tools and their versions do not change (`cddl` 0.12.14, `cbor-diag` 0.11.8); the check also needs `ruby`, which the gems already need.

## Consequences

- `spec/cddl/jdp.cddl` gains `int64`, and `raw-value` and `raw-sample` use it. Seed 6 of the list reaches the old `int .size 8` rule, so the check fails on the old file on every run.
- Changing the CDDL can change which instances a seed draws: a seed that fails after a change of the file points at that change.
- A failure of the check is reproducible on a developer machine with the same seed and the same Ruby version: how Ruby maps its random stream to integers may change between Ruby versions, so CI is the reference.
- `docs/errata/spec-tests-2026-10.md` records the fix (S-4).
