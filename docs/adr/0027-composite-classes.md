# ADR 0027 — Composite classes end to end

**Status:** accepted (v1) · **Date:** 2026-09

## Context

A class could already have members that are other classes (`member pumps: std/pump@1[1..8]`), with a maximum depth of 3. But the rest was missing: how a class member is bound in an instance, which paths it gains, whether an instance can be in two compositions, whether a method can call the members' methods, how health is computed, how a single profile fulfills a composite class, and how a Joker Micro exports one. Without this, a whole piece of equipment (a heat pump, the bench I/O node) cannot be abstracted into a single instance.

## Decision

1. In the `map` of a composite instance, a class member is bound to the **name of an instance**, and a collection to a **list** of names, checked for class, major, capabilities and limits (`J0650`–`J0654`).
2. A class member creates **nested paths** `<instance>.<member>.<submember>`, which are aliases of the same signals (16 P6).
3. An instance can be in several compositions; its `out` members still have a single envelope and a single arbiter, and `jk check` warns when two apps reach the same output through different paths (`J0655`).
4. A method can call methods of **fixed members** (`call drive.stop()`), never of collections (`JC0502`–`JC0504`). In a **virtual** instance, the members belong to the implementation: whoever calls only reads (`J-E0507`, `J0656`).
5. The health of a composite is the worst of its rules and that of the required members.
6. A profile fulfills a composite class with `[implements."<class>".members.<member>]`, recursively.
7. Joker Micro exports the tree in the `HELLO` catalog, and the Linux node sees the same paths.
8. The composition depth (≤ 3, `JC0501`) and the depth of implementation layers (≤ 3, `J0620`) are counted separately.
9. **A class describes a role, not a chip:** `bench_io@1` ("the bench I/O node") is acceptable; `esp32@1` is the mistake.

The detail is in document 14 (§2, §4.6, §9.3), in document 17 (§3.2, §4, §7, §8), in 16 §6, in ref/01 §5.13, in ref/03 §2.2.14, in ref/04 §4.8.6, in ref/05 §14.6, in ref/07 and in ref/09 §5.11.

## Consequences

- A whole piece of equipment can be an instance, even on a Joker Micro, and the Linux node sees the role, not the chip.
- `joker:meta`, the Joker Link catalog and the API now describe trees of instances.
