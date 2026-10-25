# ADR 0077 — SunSpec discovery in the Modbus drivers, its effective map, and the profile generated from it

**Status:** accepted (v1) · **Date:** 2026-10

## Context

SunSpec is the round 1 bench case and a Phase 1 exit criterion (docs/11, AUR-400). ref/03 §2.2.15 describes `[discovery]` and the embedded `joker/sunspec@1` profile, ref/06 §2.11 says discovery runs in `connect` and reports the map through `connect-info.discovered`, and ref/02 §2.1.11 and §3.1.5 pin the map in the lock and name its drift `J0401 kind=map`. ADR 0064 refuses `[discovery]` (`J0807`) because none of it was implemented. Building it leaves questions the references do not settle:

1. **The walk.** Which reads, in which order, what ends the chain, and what an equipment that answers an exception means.
2. **The effective map.** ref/02 says the digest covers "signals, addresses, scales" and lists `models` as the driver field, but not what the map is made of.
3. **Scale factors.** ref/03 gives the `*_SF` points as `scale_factor`, which this runtime does not read (ADR 0064).
4. **Who refuses a changed chain.** The plan reads the addresses of the pinned map; on a different chain they are other registers. ref/05 §12 reads values "normally" under drift.
5. **The generated profile.** AUR-400 asks for a profile in the format of `jk profile import`; ref/03 has no rule for its paths when a model repeats.

## Decision

### Discovery

1. `modbus-tcp` and `modbus-rtu` declare the capability `discover`. When the device's quirks carry `discovery` (ref/06 §2.4.2: `kind`, `bases`, `models`, `max_models`), `connect` walks the chain after connecting; a `kind` other than `sunspec`, an unknown key or an empty `bases` is `J0825` when the driver opens.
2. **The walk** reads only, with FC 3:
   - the marker `SunS` (`0x5375 0x6E53`) at each address of `bases` in order (default `[40000, 50000, 0]`); an exception or other registers move to the next base; no base with the marker is `J0876`;
   - from base + 2, the header of each model (`id`, `len`); the next header is at `at + 2 + len`; the id `0xFFFF` ends the chain, and so does an exception on a header (equipment that omit the end); more models than `max_models` (default 64), or a model past address 65535, is `J0876`;
   - the body of each accepted model whose definition the driver has and whose length fits one request (≤ 125 registers);
   - a timeout or a broken link on any read is `J0876`.
   A model outside `models` is walked over and left out of the map.
3. **Built-in models.** The driver knows model 1 (common), the integer inverter models 101–103 and the fixed head of the integer meter models 201–204 (currents to `TotWh_SF`). The identity is model 1: `Mn` → `vendor`, `Md` → `model`, `SN` → `serial`, `Vr` → `firmware`.

### The map, the lock and drift

4. **The effective map** is the deterministic CBOR `{ models: [{ id, at, len }, …], scales: { "<prefix>.<sf point>": integer, … } }`: the accepted models in chain order, `at` the address of the id register, and every scale factor read. The prefix of a model is `m<id>`, and `m<id>_<n>` for its n-th repetition (n ≥ 2). `connect` reports it with the identity through `ConnectOut::device`, and its digest (`sha256:` and the hex of SHA-256 over those bytes) is the `digest` of `[discovered.<d>]`, whose driver field `models` is the same list. A scale factor that changes is therefore a drift of the map.
5. **A changed chain fails `connect`.** When `device-spec.discovered` pins a map and the one found has another digest, the driver still reports what it found and `connect` fails with `J0401` (`kind=map`): the link is `down` and the points `bad(comm_failure)`, instead of values read from other registers. Identity, firmware and serial drift stay the core's verdict (ref/02 §3.1.5), on the reported identity.

### The generated profile

6. From a map, the driver crate generates a profile in the format of `jk profile import` (ref/03 §2.2.19): a `modbus` profile with one input per point of each built-in model, at its absolute address, with FC 3, its raw type, its unit (`%` for power factor, `°C` for temperatures) and its SunSpec name as `label`; the operating state of 101–103 is the enum `InverterState`; `bitfield32` points are `bits32`; string points are identity, not signals. Paths are `<prefix>.<point in snake_case>`, or the `[discovery] names` entry of `"<model>.<point>"`.
7. Because this runtime does not read `scale_factor`, the scale factor read at discovery becomes the point's `scale` (10^sf), with a `# REVIEW` line naming it; a "not implemented" (`0x8000`) or unusable (|sf| > 10) factor leaves the point unscaled with a `# REVIEW` line. Rule 4 makes a later change of the factor a drift. A model the driver does not know becomes a `# REVIEW` line with its address and length.

### What is not covered yet

8. jokerd does not yet hand the lock's `[discovered.<d>]` to the drivers (`device-spec.discovered`), record what `connect` reports for `jk lock --node`, raise the `joker.device.<d>.drift` alarm, or embed `joker/sunspec@1`; `[discovery]` in a profile stays refused (`J0807`, ADR 0064) until it does, and `jk discover sunspec://` has no command yet. Repeating blocks, the float models (111–113, 211–214) and the 700 series are not built in.

## Consequences

- ref/03 §2.2.15 states rules 2, 4, 6 and 7; ref/06 §2.11 states rules 4 and 5.
- `joker-driver-modbus` gains `sunspec` (the walk, the built-in models, the map, the lock entry, drift, the generated profile and a register image of a simulated device) and depends on `sha2`; `joker_hal::testing::Bench::connect_reports` returns what `connect` reported.
