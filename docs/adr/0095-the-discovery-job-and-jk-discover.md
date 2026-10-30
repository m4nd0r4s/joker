# ADR 0095 — The discovery job and `jk discover sunspec://`

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0077 built the SunSpec walk in the Modbus drivers and left `jk discover sunspec://` without a command (§8). AUR-400 needs it: profiling a PCS starts by asking the equipment what it is. ref/09 §5.17 defines `POST /v1/discovery` as a `discovery` job whose result "lists what it found (identity, SunSpec models, suggested profile, digest of the map)", but neither ref/09 nor the OpenAPI gives that result a shape. ref/06 §2.11 puts network scans in a driver instance with the `scan` capability, which no catalog driver has in 1.0. Two more points are open:

- **The rate.** ref/09 gives `DiscoveryRequest.rate` a default of 10 requests per second; ref/06 gives the driver scan instance 20.
- **The deadline.** `jk` already has a global `--timeout`, the deadline of each request to the node, so the discovery's own deadline needs another name.

## Decision

1. **What 1.0 discovers.** `POST /v1/discovery` accepts `sunspec://<host>[:<port>][?unit=<u>|<a>..<b>]` (ref/03 §2.2.20). The host is one host, and the unit is one id or a range, walked in order. Any other scheme is `J-E1708`, with the message that the driver does not scan in this runtime: `modbus-tcp://` network scans wait for the `scan` capability (ref/06 §2.11). A URI with credentials, unknown parameters or a network as the host is `J-E1708` too.
2. **How it runs.** The job runs in `jokerd`, outside the cycle domain, on a thread of its own. It is the walk of ADR 0077 rule 2, over a Modbus TCP connection of its own:
   - FC 3 only, one request at a time;
   - no faster than `rate` (default **10 requests per second**, as in ref/09, from 1 to 1000), until `timeout` (default 60 s);
   - the discovery defaults of ADR 0077 (`bases` 40000, 50000, 0; every model accepted; 64 models at most).

   A second `discovery` while one runs is `J-E1705` (409). The job never writes to the equipment, and needs no generation.
3. **`DiscoveryResult`** (the `result` of the job, in the OpenAPI) is `{ uri, hits, misses }`.
   - Each **hit**, a unit that answered the walk, has:
     - `address` (`host:port`) and `unit`;
     - `identity` (`vendor`, `model`, `serial` and `firmware`, from model 1) and `base`;
     - `models`: every model of the chain, each with `id`, `at`, `len`, `known` (the runtime has its definition) and `accepted`;
     - `digest`: the digest of the effective map, ADR 0077 rule 4;
     - `profile_name` and `profile`: the suggested profile of ADR 0077 rules 6 and 7, with the TOML as its text;
     - `lock`: the TOML of the `[discovered.<device>]` entry for that map.
   - Each **miss** has `unit`, `code` (`J0876`) and `message`.
   - The suggested name is `<vendor>/<model>@1` in lowercase with hyphens (`local/sunspec@1` without an identity). The device of the lock entry is the model in the same form, with underscores. Its driver is `modbus-tcp`, and its `at` is the instant of the walk. The user renames both when they enter the manifest.
4. **`jk discover <uri> [--unit] [--rate] [--for <duration>] [--out <file>]`.**
   - It starts the job on the node and follows it (`GET /v1/jobs/{id}`).
   - It prints each hit (identity, models, map digest, suggested profile and lock entry) and each miss.
   - `--out` writes the suggested profile of a single hit, and never over an existing file (exit code 1).
   - `--for` is the discovery's deadline; the global `--timeout` stays the deadline of each request.
   - The exit code is 0 with at least one hit, and 4 (ref/09 §9.2, no connection) when no unit answered.
   - `--json` prints the `DiscoveryResult`.

## Consequences

- The OpenAPI gains the `DiscoveryResult` schema, and ref/09 §5.17 points to it.
- Pinning what discovery found stays `jk lock --node`. The remaining items of ADR 0077 §8 are still open: `jokerd` handing `[discovered.<d>]` to the drivers, `POST /v1/lock/probe`, the drift alarm, and the embedded `joker/sunspec@1`.
- A driver that implements `scan` later answers the other schemes through the same endpoint and the same result.
