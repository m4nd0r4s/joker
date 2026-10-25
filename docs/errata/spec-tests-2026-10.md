# Errata found by `joker-spec-tests` (October 2026)

Task 0a (docs/13 §4, ADR 0080) compiled the references for the first time. These are the errors it found that were fixed in the same change, and the divergences it recorded for triage. Not normative.

## Fixed

| # | Where | Error | Fix |
|---|---|---|---|
| S-1 | `spec/cddl/agent.cddl`, ref/10 §2.1 | the rules `time` and `bytes` redefine the prelude of RFC 8610, so no CDDL tool accepts the file | renamed `timestamp` and `binary`, in the file and in the type column of ref/10; the encodings do not change |
| S-2 | `spec/cddl/joker-link.cddl` | the rule `time` (the body of `TIME`) redefines the prelude | renamed `time-msg`; the message and its encoding do not change |
| S-3 | generated manifest schema (`joker-manifest`) | the alternative forms of a key were `oneOf`, so a value valid in two forms (a quantity literal is also an expression) was rejected | `anyOf` |

## Recorded for triage

The divergences below are in `crates/joker-spec-tests/known.toml`, each scope with its issue; CI fails when one appears or disappears without the file changing.

| Check | Scope | Issue | What |
|---|---|---|---|
| Rust | ref/01 §12 | AUR-429 | the Rust model of the manifest names types that `joker-manifest` does not have |
| Rust | ref/04 §4, ref/05 §20 | AUR-284 | the Rust SDK does not have most of the surface of ref/04 §4 yet |
| Rust | ref/04 §6 | AUR-241 | the test harness of ref/04 §6 |
| Rust | ref/06 §3 | AUR-430 | `Scanner`, `Cx::locked`, `Cx::spawn_blocking`, `AdcRead`, `CanFrames` and the `DriverEntry` registry of §3.8 |
| Rust | ref/08 §2 | AUR-315 | the `joker-micro` crate does not exist |
| TypeScript | ref/05 | AUR-387 | the SDK declarations use types they never declare (`CycleView`, `LocalCycle`, `Series`, `Trace`, …), and `History<T>` does not extend `readonly Sample<T>[]` |
| CDDL | ref/07 §9.3 | AUR-431 | the `ota-begin` example carries the text `<COSE_Sign1>` where the CDDL requires an embedded `COSE_Sign1` |
| CDDL | ref/10 §2.6.7, §2.10.3 | AUR-431 | `timestamp = int / tdate`, and `tdate` is a tag, which JSON does not have: the JSON examples do not validate |
