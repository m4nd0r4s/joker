# ADR 0099 — How the plan of a generation classifies its changes

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 01 §15 says what the minimal diff must achieve: an envelope swaps without restarting the driver, the address of a device restarts only that driver, and unaffected apps keep running. Ref/09 §5.13 defines the `Plan` (`changes` with `kind` and `effect`, `disruption`, `requires_confirm`, `confirm_reasons`), and ADR 0015 makes the window mandatory in `prod` when the plan changes the network, security, the uplinks or the API. No document says which keys of the manifest make which change, and the node served no plan (AUR-270).

## Decision

1. **What is compared.** The plan compares the effective manifest of the generation with that of the generation it replaces: the active one for a generation that is not active, or the one its last activation left. A generation with nothing to replace (the first one) has no plan.
2. **Classification**, by section of ref/01 §5:

| Section | Kind | Effect |
|---|---|---|
| `[outputs.*]` | `envelope` | hot |
| `[alarms.*]` | `alarm_add`, `alarm_remove`, `alarm_change` | hot |
| `[apps.*]`, only `params` changed | `app_params` | hot |
| `[apps.*]`, otherwise | `app_add`, `app_remove`, `app_update` | reload |
| `[devices.*]`, `driver`, `bus`, `addr`, `address`, `host`, `port`, `unit`, `path` or `profile` changed | `driver_restart` | restart |
| `[devices.*]`, otherwise | `driver_reconfigure` | reload |
| `[devices.*]` added or removed | `device_add`, `device_remove` | restart |
| `[buses.*]` | `driver_restart` | restart |
| `[cycles.*]` | `group_add`, `group_remove`, `group_change` | restart |
| `[signals.*]` | `signal_add`, `signal_remove`, `signal_retype` | reload |
| `[instances.*]` | `instance_add`, `instance_remove`, `instance_change` | reload |
| `[security.api]`, `[security.roles]` | `api` (reason `api`) | reload |
| the rest of `[security]` | `security` (reason `security`) | reload |
| `[telemetry.uplink]` | `uplink` (reason `uplinks`) | reload |
| `[net]` | `network` (reason `network`) | reload |
| `[calc]`, `[bridges.*]`, `[clock]`, the rest of `[telemetry]` | `calc`, `bridge`, `clock`, `telemetry` | reload |
| `[resources]` | `resources` | restart |
| anything else | `other` | reload |

3. **Disruption:** `none` without changes; `hot` when no change reloads or restarts; `partial` otherwise. A plan never says `full`, because a generation with nothing to replace has no plan.
4. **The window.** `requires_confirm` is true when the target generation is in `prod` and the plan has a confirmation reason. `activate` without `confirm_window` is then `J-E1610`.

## Consequences

- `GET /v1/generations/{gen}` and the answer to `POST /v1/generations` carry the plan.
- The classification states what the swap is meant to do. A swap that restarts more than its plan says (today the node swaps every group at once) is a gap in the swap, not in the plan. The hot swap of envelopes and parameters is AUR-270's next step.
