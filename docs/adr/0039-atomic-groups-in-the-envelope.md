# ADR 0039 — Atomic groups in the envelope

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 02, §7.3, and ref/01 §5.16 define atomic groups (M20): the outputs of a group "all pass or none pass" in step 7 of the envelope. With `on_rate = "proportional"`, if the rate limits one member to a fraction of its requested step, every member is limited to the same fraction. The group may also name a `commit` register that only the core writes, after the group's writes. Implementing it (document 13, task 11) needs facts the text leaves open:

- which per-member results count as "passing";
- what the members apply and which outcome they report when the group does not pass;
- how the fraction is computed, and which outcome a member scaled down by it gets;
- when the commit register is written.

## Decision

1. **When.** Step 7 runs after steps 0–6 of every member, in the commit of the owner group, which is the same for every member (`J0272`).
2. **Passing.** A member with an active writer passes when its result is `accepted`, `clamped_range`, `clamped_rate` or `suppressed`. It fails when its result is `rejected(…)` or `tripped`. A member without an active writer is going to `safe`, which is always admissible: it neither passes nor fails, and step 7 does not change it.
3. **A group that does not pass.** Each failing member keeps its own outcome and applies what its own step prescribes (`on_interlock`, `safe`, trip, or hold). Each passing member applies its last applied value and gets `rejected(atomic_group)`.
4. **Proportional rate.** For each passing member with an active writer, the requested step is the value after the range step minus the last applied value, and the allowed step is the value after the rate step minus the last applied value. A member whose requested step is zero, or that was suppressed, has fraction 1; any other member has fraction allowed ÷ requested. The group's fraction `f` is the smallest fraction of its members. When `f < 1`, every member with a non-zero requested step applies `last + f × requested`. Its outcome is `clamped_rate`, unless an earlier step of that member already decided `clamped_range` (ADR 0033 §6). With `on_rate = "independent"`, each member keeps its own rate step.
5. **Commit register.** When at least one member applies a value different from its last applied value in the tick, the core writes `commit.value` to the `commit.signal` slot in the same commit. The driver's output stage sends it after the members, in `order` (document 02, §7.3). The register has no arbiter, no envelope and no decision records, and it is not traced as an output.
6. **Outcomes per member.** The trace and the decision log record each member as an ordinary output, with the outcome decided above. The group itself has no record of its own.

## Consequences

- A group either moves together or keeps its last applied values, except members that go to `safe`, which nothing blocks (document 01, §7).
- A proportional group keeps the ratios between its members' steps, as document 08 (round 4) requires for a differential vehicle.
- `rejected(atomic_group)` tells a writer that its own value was fine and that another member blocked the group.
