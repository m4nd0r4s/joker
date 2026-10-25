# ADR 0085 — The runtime alarms (`joker.*`) in jokerd

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/01 §5.15 lists the runtime alarms that `jokerd` raises without a manifest declaration: `joker.output.<s>.budget`, `.unconfirmed`, `.manual_expiring`, `.manual_restored`, `joker.group.<g>.overrun`, `joker.device.<d>.degraded`, `.offline`, `.drift`, `joker.driver.<n>.crashloop`, `joker.node.*` and `joker.agent.*`. The results and qualities they come from already existed (PRs #74, #77), but nothing raised the alarms (AUR-427). docs/01 §20 also wants an alarm while the historian's disk is critical (AUR-287), with no name. The documents give the conditions, not the severities (except `medium` for the two manual alarms and `critical` for a crashloop), nor how a condition without a delay moves through the lifecycle.

## Decision

1. **Evaluation.** A thread per generation (`svc/watch`) evaluates every condition every 50 ms from what the generation already publishes, and moves each alarm through the lifecycle of docs/02 §11 with no delay: condition true → `active_unacked`; false → `rtn_unacked` or, once acknowledged, `normal`. An alarm enters the alarm book the first time its condition holds. Runtime alarms have no `Unknown`, no suppression, and `max_shelve = 24h`.
2. **Conditions and severities.**

   | Alarm | Holds while | Severity |
   |---|---|---|
   | `joker.output.<s>.budget` | the output's last result is `Rejected(Budget)` | `high` |
   | `joker.output.<s>.unconfirmed` | `verify` marks the output `uncertain(not_confirmed)` (not a device ack, ADR 0065) | `high` |
   | `joker.output.<s>.manual_expiring` | a manual level of the output ends within 15 min | `medium` |
   | `joker.output.<s>.manual_restored` | a manual level restored at startup without a `synced` clock is still held and was not renewed (docs/16 A14) | `medium` |
   | `joker.group.<g>.overrun` | the last `alarm_after` ticks of the group, or more, all started a period or more late | `high` |
   | `joker.device.<d>.degraded`, `.offline`, `.drift` | the device's link is `degraded`, `down`, `drift` | `medium`, `high`, `high` |
   | `joker.store.disk_full` | the historian's disk is critical (ADR 0081 rule 15) | `high` |

3. **Operator actions** on a runtime alarm (`ack`, `shelve`, `unshelve`) act on the book directly, with the codes of ref/09 §5.8; a shelve ends at its TTL with `shelve_expired`. The state of a runtime alarm survives a restart like a manifest alarm's (ADR 0082) and is re-evaluated by the next generation.
4. **Journal and decision log.** Every alarm event, of the manifest or of the runtime, is appended to the alarm journal (ADR 0082) and to the decision log as a record `alarm` (`id`, `kind`, `from`, `to`, `severity`, `by`, `reason`, `message`), written with `fsync` like trips (ADR 0013: alarms have their own fsync).
5. **Not raised in 1.0.** `joker.driver.<n>.crashloop` (the W and X drivers are not in the 1.0 runtime), `joker.node.config_invalid`, `joker.node.crl_expired` (`jokerd` does not read `node.toml` yet) and `joker.agent.*` (no agent). `.drift` rises only when a link reports `drift`, which no 1.0 driver does.

## Consequences

- A protection that keeps the plant safe by refusing (budget, verify, overruns) is now annunciated, acknowledged and shelved like any alarm, and the decision log keeps the transitions next to the decisions that caused them.
- `joker.store.disk_full` is a node alarm with a name, so a full disk is visible without reading metrics.
