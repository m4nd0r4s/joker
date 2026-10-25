# ADR 0079 — Data classes and egress protection (D1–D4)

**Status:** accepted (v1) · **Date:** 2026-10

## Context

Document 26, §8 draws the data protection boundary: anonymization belongs to the data platform, and Joker's job is not letting personal data leak. It lists six minimums, D1–D6, and puts D1–D4 in 1.0. Document 26, §9 asks for D1–D4 to be decided during Phase 0, "so that the key is reserved in the schema". Until now only document 26 mentioned `data_class`: no reference defined the key, the egress declarations or the codes, so 1.0 could not add them as a minor change (ADR 0022).

ADR 0028 keeps stable in 1.0 only what is implemented, and ADR 0035 gives a reserved surface its mark. The implementation of D1, D2 and D4 is a separate task (AUR-249); D3 reuses the per-recipient encryption of ADR 0024, which ADR 0035 (decision 6) moved to Phase 2.

## Decision

### D1 — the class of a signal

1. Every signal has one **data class**: `operational`, `personal` or `sensitive`, in this order of protection. The default is `operational`, so a manifest without the key means what it meant before.
2. The class is declared with `data_class` on a device signal (`[devices.<d>.signals.<s>]`, ref/01 §5.8.1) or on a `[signals."<path>"]` entry (ref/01 §5.9). A profile or a class does not declare it: whether a value is personal depends on the site, not on the equipment.
3. **Inheritance.** A derived signal (`calc.*`, `alarm.*`, the outputs and published signals of an app, the members of a virtual instance) gets the most protective class among the signals it reads: the inputs of its expression, of its alarm condition, or the bound inputs of its app. It may declare a more protective class with `[signals."<path>"] data_class`; a less protective one is `J0260`. The class is computed by `jk check` on the effective manifest and does not change at run time.

### D2 — egress is denied by default

4. Every egress declares the classes it accepts with `accepts`: the uplinks (`[telemetry.uplink.<u>]`, ref/01 §5.17.3) and the bridges (`[bridges.<b>]`, ref/01 §5.18). The default is `["operational"]`.
5. A signal that an uplink's `classes` or a bridge's `expose` selects, and whose class the egress does not accept, is `J0261`. It is an error, not a filter: a selector that silently stops sending a value would hide the decision from whoever reads the manifest.
6. The local API keeps its roles (ref/09): reading a signal on the node is not egress. Exports that leave the node as files (`jk export`, traces, diagnostic bundles) carry only `operational` signals until D3 exists; the request that asks for other classes is refused, and the omitted paths are listed in the response, never dropped silently.

### D3 — what leaves encrypted

7. Traces, exports and diagnostic bundles that carry `personal` or `sensitive` values are encrypted per recipient with the HPKE suite of ADR 0024 (the one of `spec/cddl/secrets.cddl`): one content key per file, wrapped for each recipient. The format, its CDDL and its media type are defined with the secrets work of Phase 2 (ADR 0035, decision 6), in its own ADR. Until then rule 6 holds.

### D4 — no values in logs

8. Logs, error messages, problem details of the API, diagnostics of `jk check` and the text of the decision log never carry the value of a `personal` or `sensitive` signal: they carry its path and `<redacted:personal>` or `<redacted:sensitive>`. The decision log and the trace keep the values, because replay needs them (document 26, §8): they are local data, and leaving the node is D2 and D3.

### D5 and D6

9. Retention per class and deletion with audit (D5) are 1.0.x; encryption at rest (D6) is 1.3. Neither has a key in the schema yet.

### Marks (ADR 0035)

10. `data_class` and `accepts` exist in the 1.0 schema with their final form. Until the runtime implements them (AUR-249), they are **reserved**: `jk` validates their form, and using one is `J0290`. The generated JSON schema marks them with `"x-joker-reserved": true`.
11. `J0260` and `J0261` are registered with `status = "reserved"` and `since = "1.0"`, and become active, with no other change, when D1 and D2 are implemented.

## Consequences

- ref/01 gains §6.7 (data classes), the `data_class` key in §5.8.1 and §5.9, `accepts` in §5.17.3 and §5.18, two rows in §6.6, and the codes `J0260` and `J0261` in §10.2. ref/09 §5.5 and §5.16 note the rule of exports and traces. Document 26, §8 points here.
- `joker-manifest` gains a reserved phase for keys that are in 1.0 but not yet implemented, and refuses `data_class` and `accepts` with `J0290`.
- AUR-249 implements D1, D2 and D4: the inheritance of rule 3, `J0260`, `J0261`, the refusal of rule 6 and the redaction of rule 8, with conformance cases. D3 waits for the secrets work of Phase 2.
- Lifting a reservation needs no new ADR: the implementation and its tests are enough (ADR 0028). Changing a rule of this ADR does.
