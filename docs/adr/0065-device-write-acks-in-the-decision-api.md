# ADR 0065 — Device write acks in the decision API, and what confirms a rejected output

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ref/06 §2.9 says the output stage records each `write-ack` in the decision log, that `device-rejected` and `unsupported` are `J0877` and `late` is `J0878`, and that after a `device-rejected` ack "the output stays `uncertain(not_confirmed)` until the readback". The runtime writes every write a real driver does not confirm to the decision log as a `write_ack` record (AGENTS.md invariant 8), but:

1. ref/09 §5.7's `Decision` has no kind for a device ack, and ADR 0061 rule 7 keeps `GET /v1/decisions` to the `output` and `write` records. An operator asking the API why an output did not move sees nothing, although the log has the answer.
2. The readback verification (`verify`, docs/01 §7) is not implemented, so "until the readback" never ends: an output marked `not_confirmed` would stay so for the life of the generation, even after the equipment accepted a later write.

## Decision

1. `GET /v1/decisions` also serves the `write_ack` records, as `Decision` items with `kind = "write_ack"` (a new value of `DecisionKind`, filterable with `kind`) and the object `ack`:
   - `device`: the device of the write;
   - `write_seq`: the write's sequence number on that device (`committed-write.seq`);
   - `write_kind`: `set`, `keepalive`, `force_safe`, `relinquish` or `resync` (`write-kind`, ref/06 §2.9);
   - `status`: the `write-status` of the ack (`timeout`, `comm_failure`, `device_rejected`, `not_connected`, `unsupported`, `late`, `skipped`);
   - `code`: `J0877`, `J0878` or `null`.
   Such an item has `id`, `ts`, `generation`, `output` and `kind`; it has no `writer`, `outcome`, `applied` or `active_level`, so an `outcome` or `writer` filter does not select it. The record kinds of ADR 0058 and ADR 0061 other than these are still read from the log file only.
2. A `device-rejected` ack turns the output's own signal `uncertain(not_confirmed)`, keeping its applied value (ref/06 §2.9). The cycle thread applies it, as the only writer of the output's slot.
3. The output is confirmed again, and its signal `good`, by the readback verification once it exists, by a later `ok` ack of a write of the same output (a `set`, `keepalive` or `resync` the equipment accepted), or when the core applies a new value (a new value starts unconfirmed only once its own ack says so).

## Consequences

- ref/09 §5.7 lists `write_ack` among the decision kinds and `ack` among the fields of `Decision`; `spec/openapi/joker-api.yaml` adds `write_ack` to `DecisionKind` and the `ack` object to `Decision`, in the same commit.
- ADR 0061 rule 7 is amended by rule 1.
- `jokerd` serves the records and applies rules 2–3; `jk decisions` shows an ack with its status in the outcome column.
