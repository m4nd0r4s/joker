# ADR 0057 — `jk` may embed the daemon; no library depends on it

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 13 §1 says that no **domain** crate depends on `jokerd`, so that the core, the manifest, the drivers, the host and the services never reach back into the daemon. The policy of ADR 0055 (`policies/dependencies.md` §1) made the rule checkable and wrote it as "no workspace crate depends on `jokerd`, with any kind of dependency".

The CLI of the daemon front (AUR-196, AUR-197) runs the runtime in its own process for `jk dev` (document 07: "runs the runtime, local with `sim`"), and `jk record`, `jk replay` and `jk check` use the controller, the M0 catalogue, the replay engine and the wire helpers of `jokerd`. `joker-cli` therefore has a normal dependency on `jokerd`, and the graph test of `joker-supply-chain` fails on the integrated tree. The CLI is a binary, not a domain crate: nothing depends on it, and the reason for the rule (no layer below the daemon reaches the daemon) does not apply to it. Splitting `jokerd` into a library crate under another name would only move the same edge.

## Decision

1. The rule of document 13 §1 holds as written: no library crate of the workspace depends on `jokerd`.
2. **`joker-cli` is the only crate that may depend on `jokerd`.** It is the binary `jk`, and it embeds the runtime for `jk dev`, `jk record`, `jk replay` and the local checks. No crate may depend on `joker-cli`.
3. The rule of `policies/dependencies.md` §1 and the `graph` test of `joker-supply-chain` state the exception by name (`DAEMON_USERS`); adding another crate to it needs an ADR.

## Consequences

- `policies/dependencies.md` §1 changes in the same commit as the check.
- `jk` and `jokerd` link the same runtime, so `jk dev` on the laptop runs the code that a node runs (document 07, principle 8).
- A crate that needs something from `jokerd` other than the CLI gets it moved to a library crate below the daemon, not a dependency on `jokerd`.
