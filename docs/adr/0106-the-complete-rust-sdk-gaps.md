# ADR 0106 — The gaps that completing the Rust SDK found in ref/04

**Status:** accepted (v1) · **Date:** 2026-11

## Context

AUR-284 completes `joker-sdk`, `joker-sdk-macros` and `joker-test` against ref/04 §4 and the `peak_shaver` app of ref/05 §20.1. Writing the code found four places where ref/04 and `spec/cddl/joker-meta.cddl` cannot be implemented as written, or leave a choice open:

1. Ref/04 §4.5 declares `pub struct TaskIo<'a, A: App>`, and §4.3 writes every async task as `async fn f(io: &TaskIo<App>)`. With a lifetime parameter, that signature does not compile: an `async fn` cannot elide the lifetime of a path (`E0726`), so every task would have to write `&TaskIo<'_, App>`, which no example in the documents does.
2. Ref/04 §4.10 allows `UtcSeconds` as a field of a `Record` (`J-E0721` lists it), but `field-type` in the CDDL has no code for it.
3. The `joker-state` blob (CDDL, ref/04 §4.12) carries "the CBOR of the task state" per task, without saying how that state is encoded. The default migration "matches the fields by name", which a record encoding by position (D2, ref/04 §4.10) cannot do once fields are added or removed.
4. Keys 14 (`state-info`) and 15 (`has-on-params`) of `joker-meta` come from `#[joker::state]` and `#[joker::on_params]`, but the merge rules (ref/04 §5.3) only take the app keys from the `app-fragment`, and `#[joker::app]` cannot see the other two macros, which sit on other items. No fragment can carry those keys.

## Decision

1. **`TaskIo` has no lifetime parameter:** `pub struct TaskIo<A: App>`, with `Deref<Target = A::Task<'static>>`. The async view lives as long as the async instance, and its handles borrow nothing shorter. `CycleIo<'a, A>` keeps its lifetime: the cycle view borrows the image of one tick. Tasks are written `async fn f(io: &TaskIo<App>)`, as in the examples.
2. **Field type 16 is `utc-seconds`:** a `UtcSeconds`, encoded as a CBOR integer (seconds since the Unix epoch).
3. **A persisted state is a CBOR map keyed by field name** (text keys, in the order of the fields), each value encoded as its record field (ref/04 §4.10). The default migration reads the fields that are present by name and keeps `Default` for the others; `StateBlob::field` reads by name. Records (Internal values, events, mailboxes) keep their encoding by position.
4. **Keys 14 and 15 are optional and only informational.** The Rust SDK does not write them. A reader does not infer anything from their absence. The host does not need them: it always calls `on-params` (a component without a handler accepts the change) and `export-state` on a hot swap. An empty `export-state` result means "nothing persisted", and `migrate` with an empty blob does nothing.

## Consequences

- Ref/04 §4.2 and §4.5 name `TaskIo<App>`; §4.12 states the encoding of a persisted state and the empty blob; §5 states that keys 14 and 15 are informational.
- `spec/cddl/joker-meta.cddl` adds field type 16 and notes keys 14 and 15.
- A tier 2 SDK that can write keys 14 and 15 may still do so; `jk inspect` shows them only when present.
- Renaming a field of a persisted state loses its value under the default migration, as it did before; a custom `#[joker::migrate]` covers renames.
