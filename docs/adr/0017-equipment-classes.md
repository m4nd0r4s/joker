# ADR 0017 — Equipment classes and instances, on top of signals

**Status:** accepted (v1) · **Date:** 2026-09

## Context

Typed ports (`Output<bool>`, `Input<f64>`) are the right base, but an app with 15 pieces of equipment of 5 members each ends up with 75 loose ports, with no structure, and the business logic gets mixed with details (edges, write groups, the quality of each member). There are two audiences with opposite needs: whoever builds abstractions wants the low level, and whoever writes business logic wants `pump.start()`.

## Decision

- Versioned **classes** (`led@1`, `pump@1`, `inverter.pv@2`) defined in `.jkc` (document 17, ADR 0018) in a registry: members (input, output, param, action, event), optional capabilities, methods (declarative assignments to outputs, with atomic groups) and queries (expressions evaluated by the core).
- **Instances** of three kinds: implicit (a profile declares `implements`), composite (the manifest brings together members of several devices) and virtual (an app implements the class, with the commands as virtual outputs subject to arbiter and envelope).
- In the SDK: `#[device] led: Led`, `Many<Pump, 1, 8>`, local classes with `#[derive(Class)]` and binding by path template.
- **Everything compiles to ports and signals.** The signal layer remains complete and accessible (`raw()`).

## Reasons

- Business logic stays in domain terms, portable across sites and vendors.
- The logical equipment (a pump) is rarely a single communication device; composite instances model that without code.
- Active implementations give the abstraction author a place for reusable behavior (sequences, protections), with safety at each layer.
- A single expression evaluator (queries are derived signals of the core) preserves determinism and replay.

## Consequences

- One more registry to govern (the `std/` library is small, reviewed and aligned with standards: SunSpec, IEC 61850, CiA 402, BACnet).
- Each virtual layer adds up to one tick of latency, unless implementer and consumer share the group.
- Methods and queries are limited to a closed grammar; real behavior requires an active implementation.
- Rejected alternatives: inheritance, methods with free code, queries evaluated in the SDK, classes defining safety (document 14, §12).
