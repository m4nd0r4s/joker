# ADR 0096 — The JSON form of `timestamp` in `agent.cddl`, and the `ota-begin` example

**Status:** accepted (v1) · **Date:** 2026-10

## Context

`joker-spec-tests` (ADR 0080) validates the examples of the references against the CDDL files with the reference tool of RFC 8610. Two examples of AUR-431 did not validate:

- **Agent timestamps.** `spec/cddl/agent.cddl` wrote `timestamp = int / tdate`. The prelude of RFC 8610 defines `tdate` as `#6.0(tstr)`, a tagged text string. JSON has no tags, so the JSON examples of ref/10 §2.6.7 (`desired-state`) and §2.10.3 (`report`), which carry RFC 3339 strings, did not fit. The CBOR form of ref/10 §2.1 is an integer of nanoseconds, never a tag 0, so `tdate` matched neither form. Ref/10 §2.1 is clear: the CBOR form is `int`, the JSON form is RFC 3339 text with `Z`. The file was wrong, not the reference.
- **`ota-begin`.** The example of ref/07 §9.3 carried `h'3c434f53455f5369676e313e'` (the text `<COSE_Sign1>`) in key 5, where `joker-link.cddl` requires `bstr .cbor COSE_Sign1_Tagged`. The reference said it was abbreviated, but an abbreviation that does not fit the rule is not an example of the rule.

## Decision

1. `timestamp = int / tstr` in `agent.cddl`, like the other dual-form types of the file (`duration`, `digest`, `ulid`). Which alternative a message may use is the dual-form rule of ref/10 §2.1 (CBOR: `int`; JSON: RFC 3339 with `Z`), as for every other dual-form type; the CDDL does not repeat it. No encoding changes: no writer ever sent a tag 0.
2. The `ota-begin` example of ref/07 §9.3 carries in key 5 the whole COSE_Sign1 of the test vector of §9.1 (`conformance/link/ota.vectors.toml`, 246 B), so it validates, and its byte listing shows the real body (264 B; the message is fragmented as ref/07 §5.5 says when it does not fit in a frame).

## Consequences

- The two `[divergence.cddl]` scopes of `crates/joker-spec-tests/known.toml` are gone; the examples of ref/07 and ref/10 all validate.
- `docs/errata/spec-tests-2026-10.md` records the fixes (S-5, S-6).
