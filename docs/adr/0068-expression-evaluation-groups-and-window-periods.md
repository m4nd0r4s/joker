# ADR 0068 — Evaluation groups and window periods of manifest expressions

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/01 §3.7 lists every key that holds an expression and says where each one runs: the expressions of an output in its owning group (A11), `calc` and alarms "in the fastest group that consumes them, or in the implicit `calc` group (100 ms), or in the group indicated by `group`". ADR 0034 sizes every window with the period of the group that evaluates the expression. Building the runtime path from the manifest to a bound `Program` (AUR-417) left three questions without a normative answer:

1. **What "consumes" means.** A `calc` or an alarm can be read by an output's expression, by another `calc`, by an alarm, by a cycle task, and through chains of these. The text does not say which of them count, nor how a chain resolves.
2. **The period of a `permit`.** An action's `permit` (ref/01 §5.8.2) is evaluated before each invocation, not in a tick, so no group evaluates it; yet its windows (ADR 0060 keeps their state across invocations) need a period to be sized. The runtime used a fixed 1 s placeholder.
3. **Which samples an expression reads.** docs/01 §4 puts the expressions of a group at step 3, after the latch, but does not say whether the signals they read are latched by the group, nor what an expression reads when the signal is a derived signal the same group produces in that same step.

## Decision

1. **Consumers.** A group consumes a signal when one of these reads it: an expression (`interlock`, a `range` limit, `reset_when`) of an output the group owns; the `expr` of a `calc` the group evaluates; the `when` or `suppress_when` of an alarm the group evaluates; a binding of a cycle task of the group. A `calc` or an alarm without `group` runs in the fastest of its consumers' groups (smallest period; on a tie, the first group name in byte order); since the group of a consumer may itself come from this rule, the rule is applied until nothing changes. A `calc` or an alarm that nothing consumes runs in the implicit group `calc` (100 ms), which exists only when something runs in it. An alarm's `actions.trip` is not a read and does not make the tripped outputs' group a consumer.
2. **Order inside step 3.** A group evaluates its `calc` first, a `calc` after every `calc` of the same group it reads (the graph is acyclic, `J0190`), then its alarms, in name order. The output expressions run in the envelope (steps 5–6), as ADR 0038 says.
3. **Latching.** Every signal an expression of a group reads is latched by that group (it is one of the group's ports, ref/00 §4), so all the expressions of a tick read one consistent snapshot. The exception is a derived signal (`calc.*`, `alarm.*`) that the same group produces: it is read from its slot, so a reader in the same group sees the value of this tick, not that of the previous one.
4. **The period of a `permit`.** A `permit`'s windows are sized with the period of the fastest group that owns an output of the action's device; a device without outputs uses the period of the implicit `default` group (100 ms, 16 A11). The evaluations still happen at each invocation (ADR 0060 rule 4); the period only sizes the windows.

## Consequences

- ref/01 §3.7 points to this ADR for the meaning of "consumes", the order and the latching; ref/01 §5.8.2 states the period of a `permit`.
- `jokerd` builds every expression of a generation through one path (`jokerd::expr_plan`): the outputs' `interlock`, `requires_good`, `range` limits and `reset_when`, the `calc`, the alarms and the actions' `permit`, all compiled against the plan's own `SignalId`s (AUR-416) and with the period of the group that evaluates them.
- A `calc` or an alarm signal is registered as a derived signal of the group that evaluates it, so the group takes part in the registry hash (ADR 0031) like the owner group of an output.
