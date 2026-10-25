# ADR 0084 — Class instances on the node and manual method calls

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0041 put method expansion and class queries in the core, and ADR 0059 gave `jkc` a canonical contract. What turns them into instances a node runs (AUR-407) is left open by docs/14, docs/17 and ref/09:

- where the node gets the contracts of the classes it runs;
- what a query of an instance is in the registry, and who evaluates it;
- how methods are numbered, and in which group a method record expands;
- what the node does with a member bound through an adapter or a constant;
- what a call of `POST /v1/instances/{instance}/methods/{method}` (`jk call`) is to the arbiter, given that A10 requires a TTL on every manual write and a method record of ADR 0041 carries none;
- how a method call reaches the trace and the replay.

## Decision

### Contracts and instances

1. The node knows the classes of `std/` it embeds, and the project's own classes travel with the generation as their canonical contracts, `contracts/<package>/<name>@<version>.contract.json` (media type `joker.class.contract.v1+json`), which `jk` compiles from `classes/`. The node verifies both hashes of each file (ADR 0059 rule 17) and checks and plans the generation against these contracts.
2. The instances on a node are the composite instances of `[instances]` (with `map`; virtual and simulated instances are not run by this ADR) and the implicit instance of every device whose profile has `[implements]` (docs/14 §5.1). An implicit instance fulfils all the classes of its profile as **one** instance (docs/17 §4.5); its methods are those of its classes in reference order, and when two classes declare a method with the same name, the first one is the instance's.
3. A member is reached through its path `<instance>.<member>`, the alias the manifest checks register for a member bound to a signal (ref/01 §5.13, ref/03 §2.2.14). A method or query that reads or writes a member without one (bound through an adapter, a constant, or not bound) is refused when the generation is planned, with `J0807`; so is an assignment to an output whose unit differs from the member's, because a method program writes the value in the output's own unit (ADR 0041 rule 3).

### Queries

4. A query of an instance is the derived signal `<instance>.<query>`, evaluated like a `calc`: one owner group chosen by ADR 0068, in dependency order, with the expression's quality (ADR 0041 rule 5). A query path equal to an existing path is `J0604`. Queries of a capability the instance does not have do not exist on it.

### Methods

5. A method's number is its position among the instance's methods (rule 2: the contract's order, by name, class after class). An instance's method records expand in the group that owns the `out` members its methods write; an instance whose methods write outputs of more than one group is refused with `J0807`.
6. **Manual callers.** A method record carries the TTL of a manual caller (`MethodCall.ttl_ns`), and every write the call expands to carries it, so the arbiter holds the caller's level for that TTL (docs/16 A10) as it does for a direct manual write. For any other caller the TTL is 0 and the writer's lease applies. Like the TTL of a direct manual proposal, it is not recorded in the trace.
7. **`POST /v1/instances/{instance}/methods/{method}`** (ref/09 §5.11, `jk call`) requires `operator`, and calls the method as the manual writer of the level asked (default `manual:4`), with the TTL, reason, takeover and wait rules of a manual write (`J-E1101`–`J-E1106`). The outputs the method writes, its nested calls included, are known when the generation is planned: each one gets the manual occupancy of a manual write, and the takeover rule applies to each. The record is staged in the owner group's async method inbox (step 2) and expanded in step 5a. The reply is a `MethodCallResult` with one `WriteTicket` of `kind = "method"` per output; a call that expands to nothing is `422` with the code of the core (`J-E0504`, `J-E0505`); an unknown instance is `J-E1410`, an unknown method `J-E1411`, a missing, unknown or invalid argument `J-E1412`.

### Trace and replay

8. Each tick's trace records its direct proposals and its method records apart (ADR 0043), never the writes the records expanded to. The replay stages again the recorded method records of every writer that is not a cycle task (whose records its task makes again) and expands them, so a recording with method calls replays to the same bytes.

## Consequences

- A `std/led@1` bound in `[instances]`, and the bench ESP32 through its built-in profile, take `jk call led_pi.toggle --ttl 10m --reason …` end to end: API, method record, expansion with `own()`, arbiter, envelope and the output.
- `button.clicked` is a signal any app, alarm or `calc` can read.
- Adapters of `[implements]` and of `map` (`scale`, `enum_map`, `edge`, `const:`) still have to reach the runtime before the members they bind can be used by methods and queries; virtual instances, events and the instance views of `GET /v1/instances` remain to be built.
