# ADR 0024 — Secrets encrypted per recipient, versionable in Git

**Status:** accepted (v1) · **Date:** 2026-09 · **Replaces** the rule "`secrets.toml.enc` never in git" (documents 03 §1 and 15 §2.4)

## Context

The previous design encrypted secrets "with the device key" and forbade the file in Git. That does not say how a secret reaches 3,000 nodes, nor how the Deck delivers it without being able to read it, nor how a credential is rotated without touching the configuration. And a file outside Git is a file that gets lost.

## Decision

1. Each secret is encrypted with **HPKE** (RFC 9180) to the KEM key (X25519) of **each recipient node**, attested in the node's certificate, and signed per entry, with a revision (`rev`) that never goes down.
2. `secrets.toml.enc` **may go into Git**: it only contains encrypted values, and that is how the Deck distributes them without being able to read them.
3. Rotating a secret does not create a new generation. **Exception on MCUs:** the secret goes inside the generation blob, and rotating it produces a new blob with the same manifest (new `gen_seq`, same Linux generation and same `source_hash`); the rule is in ref/08 §2.17.
4. Apps ask for secrets by a local name, bound in the manifest: `[apps.<app>.secrets] <local_name> = "secret:<name>"`. Only in the async instance.
5. No interface exposes the value: the API, the report to the Deck and the trace show names and revisions (ref/00 §9).

The detail is in ref/02, §2.4 and §3.4.

## Reasons

- Distribution across the fleet becomes the same as that of the configuration, without a side channel.
- A compromised Deck does not read secrets.

## Consequences

- Enrolling a node also generates the KEM key (ref/02, ref/10).
- A new node only receives a secret after someone with the `secrets` key encrypts it for it (`jk secret recipient add` and `jk secret push`, ref/02 §2.4).
