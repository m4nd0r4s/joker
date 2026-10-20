# joker-hal

The contract between the core and a driver (ref/06 §2–§3): the records of every tier, the `Planner`, `Driver` and `RtPart` traits, the read plan, the host services (`Cx`), the transports (`Link`) and the committed-write channel from the cycle domain to the drivers. `no_std`, with `alloc`, `std` and `plan` as features (docs/13 §1).

A driver only ever receives a `CommittedWrite`, and a `CommittedWrite` can only be built by `commit::CommitRx` from the values applied by the envelope or from the output's declared `safe` (invariant 3, ADR 0005, ADR 0047).

The M0 subset is described in ADR 0047.

## Features

| Feature | Adds |
|---|---|
| `alloc` | owned plan requests, `Plan`, plan validation, sample buffers, CBOR writing, `Link::Serial` |
| `std` | `Link::Paths`, the tokio host clock and the serial transport (`std_host`) |
| `plan` | the `Planner` trait |
| `testing` | a virtual-time host and executor, an in-memory serial line, a bench scheduler and a real envelope for driver tests |
