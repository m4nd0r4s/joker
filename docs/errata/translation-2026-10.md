# Translation errata (October 2026)

This file collects every ambiguity and erratum that came up while translating the specification to English (ADR 0029): the ones the ten translation pull requests reported (#11, #14, #15, #16, #19, #20, #23, #24, #25, #26), and the ones the integration pass (AUR-175) found. It is not normative. Each entry says where the problem is, what it is, and how to resolve it.

Each entry has a status:

- **Fixed:** a pure translation or consistency slip with one obvious answer (a wrong section number, a count, spelling, a term), fixed in the integration pass. The commit is named.
- **Resolved:** a choice the translation had to make. The choice is applied across the corpus and recorded in the glossary or in an ADR; nothing is left to decide unless a reviewer disagrees.
- **Open:** a question about meaning. The text was translated as written, and the owner of the document decides. The proposed resolution is a suggestion.

Remissions follow glossary §1.5: `NN §x` is a document of `docs/`, `ref/NN §x` a reference.

## Cross-cutting

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| X1 | #11, #15, #20, #26 | glossary §15–§18 | Four branches added a section numbered 15 or 16 before Appendix A, and several added the same term with different words: `stamp`/`timestamp`, `read-back`/`readback`, `trigger-based`/`triggered recording`, `outbox`/`output box`, `resume`/`resumption`, `ML-ready`/`Ready for ML`, `SGTC` as BMS or spelled out. | One glossary: sections 15 to 18, one row per term, and the corpus follows the chosen term (commit "docs: apply the integrated glossary terms across the corpus"). | Fixed |
| X2 | #11, #23, #26, #20 | identifiers in examples (04 §3.6, ref/05 §14.3 and §17.2, track, examples, proposal) | Portuguese identifiers survived the translation because identifiers are never translated: `luz`, `pisca`, `botao`, `toque`, `sinal`, `cpu_quente`, `canario`, `estavel`, `segredos`, `carga`, `incidente.jktrace`, `decisoes.csv`, `keys/minha.pub`, `@hmi.label("Bomba")`, and others. | ADR 0036: renamed to English with one map (glossary Appendix B). | Fixed |
| X3 | #24 | ref/03 §2.2.10, ref/04 §3.2 | Two normative literals were Portuguese: `"$valor"` and `n_largos`. | ADR 0036 renames them `"$value"` and `n_wide`. | Fixed |
| X4 | #11, #15, #26 | sample `error[…]` lines in docs, track, examples and proposal | The sample diagnostics were translated by hand before `spec/codes.toml` was English, and 39 of them did not read like the registry. | Each headline now starts with the registry message, word for word; the details of the example follow after a colon (commit "docs: quote the registry message in every sample diagnostic"). | Fixed |
| X5 | integration | `spec/codes.toml` J0101, J0108, J0720, J0721, J0828, J0829 | Some registry messages carry documentation-only text: case lists (`(line, port, bus, accelerator)`), a hint about the output (`(with suggestion)`), an example (`(W vs var)`) or remissions (`(§2.11.5, ref/04 §4.14)`). Printed as a product message, they read oddly. 16 §9 already says the seed messages are descriptions and the product messages are written in task 0b. | In task 0b, write the product messages without those parentheticals, then update the sample headlines (X4) to the product text. | Open |
| X6 | integration | track 3 §4, J0108 | The registry message for J0108 is `incompatible quantity (W vs var)`, an example that is wrong for the temperature binding of track 3. The sample keeps `incompatible quantity in the binding`. | Make the J0108 message generic (`incompatible quantity`), as part of X5. | Open |
| X7 | #26, #11, #20 | 07, ref/01 §8, ref/03 §2.2.19, track 1, 2 and 6, 23, 25 | The CLI markers were translated in some documents and not in others: `# REVER` / `# REVIEW`, `# omissão` / `# default`. | `# REVIEW` and `# default` everywhere (commit "docs(cli): use the English # REVIEW marker in document 07"; ref/01 §8 already said `# default`). | Fixed |
| X8 | #26, #16, #25 | ref/07, track 9 | `HELLO(pedido)` was rendered differently by the reference and by the track. | `HELLO(request)` in both; `HELLO(offer)` and `HELLO(answer)` keep their own meaning (glossary §15). | Resolved |
| X9 | #16, #19, #25 | ref/01 §2.2, ref/07, ref/08, ref/09 | The type metasyntax of ref/01 §2.2 (`Duração`, `Quantidade<D>`, …) had to be translated the same way in every reference. | `Duration`, `Quantity<D>`, `Rate<U>`, … everywhere (glossary §16); no Portuguese type name is left. | Resolved |
| X10 | #15, #25 | 16 §7, ref/08 §2.4 and §6 | The `.jktrace` field names (`n_saídas_ML`, …) had to match between doc 16 and ref/08. | `n_ml_outputs`, `n_outcomes`, `state_segment`, `app_state` in both. | Resolved |
| X11 | #15, #20, #23, #24, #25 | 11 heading and its citations (12, ADR 0019, ref/03, ref/04, ref/05, ref/06, ref/07, ref/08, ref/09, ref/10, OpenAPI) | Document 11's heading was cited in five shapes, and as `ML-ready`. | The heading is "Ready for ML"; every citation is `document 11, §"Ready for ML"` (commit "docs: cite the Ready for ML section of document 11 the same way everywhere"). | Fixed |
| X12 | #14, #20, #24 | prose and comments in AGENTS.md, `spec/c/joker_node.h`, `spec/cddl/joker-meta.cddl`, `wit/`, ref/04 §4.16 | British spelling left over: `Synchronisation`, `initialise`, `catalogue`, `recognises`, `cancelled` in prose. | American spelling (commit "docs: use American spelling in the prose and comments left over"); identifiers such as `is-cancelled` and the enum value `cancelled` stay. | Fixed |
| X13 | #11, #15, #16, #19, #20, #23, #26 | placeholders in code spans (`<nome>`, `<placa>`, `<porta>`, `{alvo}`, `classes/<pacote>/<nome>.sim.toml`, …) | The branches translated placeholders as metasyntax; two (`<pacote>` in 11 and 14) were left. | Translated everywhere (glossary §1.4); the last two in the ADR 0036 commit. | Fixed |
| X14 | integration | other branches (ESP32 bench firmware, conformance) | Branches that are not part of this translation still use the old identifiers and file names. | Follow glossary Appendix B when they are rebased on the English specification (ADR 0036, consequences). | Open |

## Document 00 (overview)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 00-1 | #11 | DX section, J0107 | The example message differed from the registry. | Registry message (X4). | Fixed |
| 00-2 | #11 | DX section, `jk new tanque` | Portuguese CLI argument kept to match examples/00. | `jk new tank` in both (X2). | Fixed |

## Document 01 (architecture)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 01-1 | #11 | §9 | `p99,9` with a decimal comma. | `p99.9`, as in §21. | Resolved |
| 01-2 | #11 | §22, MCU rows | "1 (MCU, M1)" and "2 (MCU, M2)" collide with the primitive numbers M1 and M2. They probably mean the Joker Micro milestones. | Rename the column values to the milestone names of document 13 (for example "MCU milestone 1"). | Open |
| 01-3 | #11 | §4, tick diagram | The steps run 4, 4b, 5a, 5, so 5a comes before 5. | Confirm the order with 16 §7 and renumber (5a after 5, or 5 → 5b). | Open |

## Document 02 (signal model)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 02-1 | #11 | §11, alarm diagram | The state labels were renamed `Active·Unacked`, `Active·Acked`, `RTN·Unacked` to match `AlarmState` (ref/00 §6, ref/04), not translated literally. | Keep; the labels are the enum names. | Resolved |

## Document 03 (manifest)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 03-1 | #11 | §13 | The illustrative group `deck:acme/group:operadores` became `group:operators`. | Keep. | Resolved |
| 03-2 | #11 | §15, J0401 | `deriva de dispositivo` had no English source in the registry yet. | The registry now says "device drift against the lock"; the table follows it. | Resolved |
| 03-3 | #11 | §15, code table | The table is not in numeric order (J0380 before J0360; J0502 and J0510 after J0656). | Sort the table by code. | Open |
| 03-4 | #11 | §1 vs §5 | `[devices.sim_tank]` uses `sim/tank.sim.toml`, but the §1 file tree lists only `sim/pcs.sim.toml`. | Add `sim/tank.sim.toml` to the tree, or drop the device from §5. | Open |
| 03-5 | #11 | §6.1 | `SGTC` next to a battery `bms`. | Spelled out as "building management system" (glossary §14). | Resolved |

## Document 04 (SDK)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 04-1 | #11 | §3.6, §3.7 | Portuguese identifiers (`botao`, `nunca_descarrega_abaixo_da_reserva`, `qualidade_ma_nao_escreve`). | Renamed (X2). | Fixed |
| 04-2 | #11 | §7 | The file name `incidente.jktrace`. | `incident.jktrace` (X2). | Fixed |

## Document 05 (drivers)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 05-1 | #11 | §9 | The source parenthetical `(a do jokerd selada)` is unclear; rendered "(the `jokerd` one sealed)". | The author rewrites the sentence; it probably means the `jokerd` unit, which is sealed by its systemd sandbox. | Open |

## Document 06 (ML)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 06-1 | #11 | §5 | `jk explain pipeline inspect` does not match the 07 syntax `jk explain <key\|code\|pipeline>`. | Use `jk explain <pipeline>` (or add `inspect` to 07). | Open |

## Document 07 (CLI)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 07-1 | #11 | §1.8 | `um nó de produção não deve ter uma toolchain` rendered as "should not"; it may be "must not". | The author decides between a recommendation and a rule. | Open |
| 07-2 | #11 | §9 | `--node revpi-porto` lacks the `.local` used elsewhere. | Both forms resolve (ref/02 §3.3); write `revpi-porto.local` if the example means a host name, or keep it if it is a context name. | Open |
| 07-3 | #11 | §7, `jk board probe` | `nível` read as the board support tier. | "tier". | Resolved |
| 07-4 | #11, #20 | command table, §7 | `# REVER` marker. | `# REVIEW` (X7). | Fixed |

## Document 08 (stress tests)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 08-1 | #11 | round 5, hardware table | "1 800" used a space as the thousands separator. | "1,800" (glossary §1.3). | Fixed |
| 08-2 | integration | §2 block, round 3 table | "trigger-based recording" differed from M7's "triggered recording". | "triggered recording" (X1). | Fixed |

## Document 09 (architecture under stress)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 09-1 | #11 | attack 5 table | Cited "invariant 5 of `AGENTS.md`" for allocation during the tick; allocation is invariant 6. | Invariant 6 (commit "docs: fix wrong remissions and counts found while translating"). | Fixed |
| 09-2 | #11 | §12.2 | Said the minimal manifest has 16 lines; the block has 17 (13 without blank lines). | "17 lines (13 without the blank ones)". | Fixed |
| 09-3 | #11 | attack 8 | The text counts "seven" versioned things; the policy table adds JDP and has no runtime row. | Add JDP to the list and say "eight", or drop JDP from the table. | Open |
| 09-4 | #11 | attack 11 | `deve ser o único cliente` rendered as "should" (a recommendation of document 12). | The author decides. | Open |

## Document 10 (design changes)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 10-1 | #15 | §1 | Row M24 comes after M25 and M26. | Sort the rows by M-number. | Open |
| 10-2 | #15 | R13, M21 | Remissions "08 R2" and "09, attack 8" differ from the `08·R2d` and `09·A8` style used elsewhere. | Use the `08·R2x` and `09·Ax` style; the exact sub-letter of R2 needs the author. | Open |

## Document 12 (limits)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 12-1 | #15 | §12 | `(ref/05, 1.3)` reads as ref/05 §1.3 or as release 1.3. | Write `(ref/05; release 1.3)` or `(ref/05 §1.3)`. | Open |

## Document 14 (classes and instances)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 14-1 | #15 | §9.3, JC0501 sample | The sample message differed from the registry. | Registry message plus the depth (X4). | Fixed |
| 14-2 | #15 | §9.3 | `Com o SG-Ready em falta` rendered as "missing"; it could mean "failing". | The author decides. | Open |
| 14-3 | #15 | `spec/codes.toml` JC0501 | `doc = "17 §7"` while the depth rule is in 17 §8. | No change: 17 §7 is the code table where JC0501 is listed, and the message says "counted as in §8". | Resolved |

## Document 16 (core semantics)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 16-1 | #15 | §4 | `inteiros do mesmo sinal` rendered as "same signedness", as the examples show. | Keep. | Resolved |
| 16-2 | #15 | §7 | The `--diff` label `antes do estado` became "before the state". If it is a literal CLI string, it must match the CLI. | Define the `jk replay --diff` labels in 07 and copy them here. | Open |
| 16-3 | #15 | A13 | Manual-level occupancy wording. | "held / holder" (glossary §15). | Resolved |

## Document 17 (`jkc`)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 17-1 | #15 | §11 | "the 15 cases of the original proposal", but the parenthetical names 14. | The proposal has 15 (S1–S15); name the missing one in the parenthetical. | Open |
| 17-2 | #15 | §6 | The `├─`/`└─` tree is malformed (a `└─` followed by more branches). | Redraw the tree. | Open |
| 17-3 | #15 | §7, JC0310 | "queries in a cycle": a dependency cycle or the cycle domain? Ambiguous in the doc and in the registry. | "queries in a dependency cycle" if that is the rule. | Resolved (ADR 0059) |

## Document 18 (v1 readiness)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 18-1 | #15 | §2 | `omissões e códigos` rendered as "defaults and codes". | Keep. | Resolved |
| 18-2 | #15 | §5, §6, §7 | §5 says the accepted ADRs are 0001–0018, while §7 cites 0019–0023; §6 says "four" missing decisions, but the license is already checked and §7 adds a fifth. | Update the counts to the current ADR index, or mark the section as history. | Open |
| 18-3 | #15 | P7 | Unclear which track (the old 8-part one or the current 11-part one) the walk-through used. | The author states it. | Open |

## Document 19 (Joker Micro)

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| 19-1 | #15 | §4.2 | "the integrator calls it" can refer to `jk_service` or to `jk_link_poll`. | Name the function. | Open |
| 19-2 | #15 | §2 | The ASCII boxes are uneven (104–106 columns), as in the source. | Redraw the boxes. | Open |

## Documents 20–26 and ADRs

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| A-1 | #20 | ADR 0005, consequences | Cited "Invariant 2 of `AGENTS.md`"; the envelope rule is invariant 3. | Invariant 3. | Fixed |
| A-2 | #20 | ADR 0009, status line | "where it says 'Phase 2', read 1.1", but the body already says 1.1. | Drop the note, or keep it as history. | Open |
| A-3 | #20 | ADR 0019 | Quotes the doc 11 heading. | "Ready for ML" (X11). | Fixed |
| A-4 | #20 | ADR 0015, note | `trinco de transição de gerações`. | "generation-transition mutex" (glossary §6). | Resolved |
| A-5 | #20 | ADR 0006 | `SGTC`. | "BMS (building management system)" (glossary §14). | Resolved |
| A-6 | #20 | ADR 0001 | `a jk` (feminine article). | Plain `jk`. | Resolved |
| 22-1 | #20 | 22 §6 X5 | `coalescidas` (field 5 of `SAMPLES`). | "coalesced", the same word as ref/07 §6 and `spec/cddl/joker-link.cddl`. | Resolved |
| 22-2 | #20 | 22 §2 diagram | The source box lines are off by one column on some rows. | Redraw. | Open |
| 23-1 | #20 | 23 §2 diagram | "(Fase 2: link UDP)" became "Phase 2: UDP link" without parentheses, to fit the box. | Keep. | Resolved |
| 21-1 | #20 | 21 §8.3 | `trilha própria` means a separate product track, not the learning track. | "a separate track". | Resolved |

## ref/00, ref/01 and ref/02

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| R01-1 | #16 | ref/01 §2.2 | The `Percentage` example is `"20 %"`, with a space, against glossary §1.3. | It is a literal: say in ref/01 §2.2 whether the parser accepts both forms, and use `"20%"` if not. | Open |
| R01-2 | #16 | ref/01 §3.7 | The `models.*.shadow.promote_if` row has "KPI grammar" in the Result column. | Give the type (`Expr<bool>` or the KPI grammar's name) like every other row. | Open |
| R01-3 | #16 | ref/01 §5.10 | `J0239` covers both the `schedule_depth` range and "driver without timed outputs"; the origin of `widen` is "03 §2". | Split J0239 into two codes (16 §9: one severity, one meaning); check the origin of `widen`. | Open |
| R01-4 | #16 | ref/01 §5.13 | `J0193` is used for an unknown class and for a capability the class lacks. | Split, or say in the message that both are covered (the registry already says "unknown class or capability"). | Open |
| R01-5 | #16 | ref/01 §5.16, `on_rate` row | Validation says "02 §7.3" and Origin "03 §10"; every other row has Origin 02 §7.3. | Validation "—" (or the actual check) and Origin "02 §7.3", after checking 03 §10. | Open |
| R01-6 | #16 | ref/01 §7.4, 2nd bullet | The parenthetical about the `micro` key and the MCU manifest's envelope is garbled in the source. | The author rewrites it. | Open |
| R01-7 | #16 | ref/01 §8.2 vs §7.4 | §8.2 says `accept_missing_fallback` "lives in the lock"; §7.4 shows the key in the manifest, matched by an `[accepted]` entry in the lock. | Say that the manifest declares it and the lock records the acceptance. | Open |
| R01-8 | #16 | ref/01 §13, case 1 | The typo example is `max_leas`, but the suggestion shown (`requires_good`) belongs to the second typo (`requse_good`). | Show both suggestions, or keep one typo. | Open |
| R01-9 | #16 | ref/01 §12 | Rust comments said "annex A" and "normalisation". | "appendix A", "normalization". | Resolved |
| R02-1 | #16 | ref/02 §2.1.3 vs §2.1.15 | `models` is listed among the sections with common fields, but the "Who writes what" table has no `models` row. | Add the row (1.1). | Open |
| R02-2 | #16 | ref/02 §2.1.14 | The only place that says "v1.0" rather than "v1". | "v1", unless the distinction is meant. | Open |
| R02-3 | #16 | ref/02 §2.6 | The range is `J0900`–`J0929`, but the table defines `J0987`; `J0939` (§2.5.5) is outside the range and not defined. | Align the range with ref/00 §8 and define J0939, or move it. | Open |
| R02-4 | #16 | ref/02 §2.4 | "(§3.10: …)" read as local but meant ref/01 §3.10. | "ref/01 §3.10". | Fixed |
| R02-5 | #16 | ref/02 §3.1.3 vs §3.1.5, §2.1.12 | `fallback.configure = true` against the lock field `configured = true`. | Use `configured` if it is the same field. | Open |
| R02-6 | #16 | ref/02 §3.1.3 | The remission "(§5.12, §5.16 of ref/09)" lists four endpoints but two sections. | Give one section per endpoint. | Open |
| R02-7 | #16 | ref/02 §2.2, layout block | "one CBOR line per activation" is odd, because CBOR is binary. | "one CBOR record per activation". | Open |

## ref/03 and ref/04

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| R03-1 | #24 | ref/03 §2.2.14 | "(or a simulation model, §2.4.8)", but `[implements]` in simulation models is §2.4.9. | §2.4.9. | Fixed |
| R03-2 | #24 | ref/03 §2.2.10 | `"$valor"`. | `"$value"` (X3, ADR 0036). | Fixed |
| R03-3 | #24 | ref/03 §2.6.1, §2.6.5 | The traceability string `"01 §7 passo 5"`. | `"01 §7 step 5"`, as 16 §8 (X2). | Fixed |
| R03-4 | #24 | ref/03 §2.4.6 grammar | The nonterminal `nome`. | `name`, like the 02 §10 grammar (glossary §16). | Fixed |
| R03-5 | #24 | ref/03 §2.9, J0816 | The reference adds "(`max_age` must be greater than the `poll`, also in the adjustments of the manifest)", which the registry lacks. | Add the condition to the registry message, or move it to `explain`. | Open |
| R03-6 | #24 | ref/03 §2.3.3, §2.9, J0830 | `[capacities]` holds fixed firmware sizes; the J0830 message says "firmware capabilities". | "firmware capacities above the board memory" (glossary §18). | Open |
| R04-1 | #24 | ref/04 §3.2 | `n_largos` in the `image-len` formula. | `n_wide` (X3, ADR 0036). | Fixed |
| R04-2 | #24 | ref/04 §4.16, J0109 and J0720 | The SDK first-line messages differ from the registry (and said "catalogue"). | The spelling is fixed (X12). Decide whether the SDK prints the registry message (as X4 does for `jk`) or its own text. | Open |
| R04-3 | #24 | ref/04 §1.2 vs §9 | "the 34 `rust` blocks" vs "the 39 `rust` blocks"; the §9 table has 39 rows. | 39 in both. | Fixed |

## ref/05 and ref/06

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| R05-1 | #23 | ref/05 §31, J-E0809; ref/06 §7, J0888, J0860, J0866 | The code tables carry more text than the registry ("stable API of a world later than the generated one…", "of an output", "(the value is saturated)", "in 1.0 and 1.1, any W driver from outside the tree, §4", "thresholds per tier in §2.12"). | Extend the registry messages (or their `explain`) and copy them back to the tables. | Open |
| R05-2 | #23 | ref/06 §2.14 | `deve usar nos timeouts do transporte` and `deve ser pura e rápida` rendered as "should"; the first may be a requirement. | The author decides between "must" and "should". | Open |
| R05-3 | #23 | ref/05 §14.3, §17.2 | Portuguese identifiers. | Renamed (X2). | Fixed |
| R05-4 | #23 | ref/06 §3.7 | `mestre` / `escravo` for Modbus. | "client / server" (glossary §7); the identifier `modbus-slave` stays. | Resolved |
| R06-1 | integration | ref/06 §2.8, quality table | "`ts-src` stamp" (noun). | "timestamp" (X1). | Fixed |

## ref/07 and ref/08

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| R07-1 | #25 | ref/07 §10.2 heading | Said `J0730`–`J0741`, but the table defines `J0742`. | `J0730`–`J0742`. | Fixed |
| R07-2 | #25 | ref/07 §2 | `max_frame` negotiated in `HELLO` "(§3.6)"; the frame sizes are §3.7. | §3.7. | Fixed |
| R07-3 | #25 | ref/07 §3.3 vs ref/01 `[buses]` | The RS-485 `turnaround` default is 100 µs in ref/07 and 3.5 characters at the baud rate in ref/01. | Pick one default and state it in both. | Open |
| R07-4 | #25 | ref/07 §6.19 | `bit 1 arquivado (*shelved*)`. | "bit 1 shelved". | Resolved |
| R07-5 | #25 | ref/07 §11.6 | Decimal commas inside formulas (`3,1`, `26,2 ms`). | Points. | Resolved |
| R08-1 | #25 | ref/08 §2.16, J0710, J0786, J0791, J0792, J0794, J0795, J0796 | The table carried more detail than the registry. | Same as R05-1: align registry and table. | Open |
| R08-2 | #25 | ref/08 §1, §2.16, §6 | The code ranges disagree: `J0770`–`J0793` (§1), up to `J0796` with `J0797`–`J0799` free (§2.16), `J0770`–`J0794` v1.0 and `J0795`–`J0799` reserved (§6). | Take §2.16 as the source and fix §1 and §6. | Open |
| R08-3 | #25 | ref/08 §2.16, J0720 | Says J0720 is also in ref/04 §4.16, while the registry points to ref/04 §4.14. | No change: §4.14 defines the restriction and §4.16 lists the SDK compile error. | Resolved |
| R08-4 | #25 | ref/08 §2.13.3, `ALARMS` | The message template placeholder `{caminho}` became `{path}`; the template syntax is not defined elsewhere. | Define the alarm message template (placeholders) in ref/01 §5.15 and reference it. | Open |
| R08-5 | #25 | ref/08 §3.7, D11 | Cites "document 18, P6" for inheritance on a generation swap, while ref/07 §8.3 cites "document 16, P6". | No change: 18 P6 is "a new generation in the middle of a ramp" and 16 P6 is "nested paths"; both are right. | Resolved |

## ref/09 and ref/10

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| R09-1 | #19 | ref/09 §5.2, `NodeHardware.cmdline` | `as linhas de arranque, separadas`, but `/proc/cmdline` is one line of parameters. Rendered "the boot parameters, split". | Confirm. | Open |
| R09-2 | #19 | ref/09 §5.9 | Informal `propose(output, value, justification, ttl)` vs `propose(p, v, reason, ttl-ns)` in `wit/joker-runtime.wit`. | Use the WIT names in the reference. | Open |
| R09-3 | #19 | ref/09 §3.12, `text/csv` row | Pointed to "§5.7 and §5.11" for the CSV columns; they are in §5.5 and §5.7. | §5.5 and §5.7. | Fixed |
| R09-4 | integration | ref/09 §6.5 | "outboxes" for the latest-wins output boxes. | "output boxes" (X1). | Fixed |
| R10-1 | #19 | ref/10 §2.17 vs §2.9 | The `poll` key validation "1 min–24 h" contradicts §2.9, where `poll_interval` also allows 0. | Allow 0 in §2.17 (meaning "push only"), or drop it in §2.9. | Open |
| R10-2 | #19 | ref/10 §3.12, item 2 | `Por isso os bundles de produção devem ter pinos` rendered as "should". | The author decides. | Open |
| R10-3 | #19 | `spec/codes.toml` J-E1120, J-E1607 | "the output is not in trip" and "rejected by the trust policy" read differently from the prose ("tripped", "refused"). | "the output is not tripped"; keep "rejected" (a policy check, glossary §1.1). | Open |

## `spec/` and `wit/`

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| S-1 | #14 | `spec/codes.toml` J-E0712, J-E0802, J-E2106, J0730 | Wording to check: "(or the async one)", "mailbox receive on the wrong side", "reverted to the previous key", "does not survive one lost renewal". | Review in task 0b with the product messages (X5). | Open |
| S-2 | #14 | `spec/codes.toml` J-E2315 | `ocupado` became "busy", not "held": the action queue or deduplication table is full, which is a busy resource, not a held manual level. | Keep "busy" (glossary §15). | Resolved |
| S-3 | #14 | `spec/codes.toml` J-E0206, J-E0308 | "cancelled" in English messages. | "canceled"; the enum identifiers stay. | Resolved |
| S-4 | #14 | OpenAPI `AgentDesired`, J0953, J0959, J0962 | `aplicação` meaning applying a generation or a desired state. | "apply" (noun), never "application" (glossary §17). | Resolved |

## Learning track, examples and proposal

| # | Source | Location | Problem | Proposed resolution | Status |
|---|---|---|---|---|---|
| T-1 | #26 | track, sample output | `[s/N] s` prompts, decimal commas and space-grouped thousands. | `[y/N] y`, points, comma-grouped thousands (glossary §1.3). | Resolved |
| T-2 | #26 | track, sample commits | The scope `trilha(NN)`. | `track(NN)`. | Resolved |
| T-3 | #26 | track 7, `jk class list` and `jk class show` | The class descriptions were translated by hand. | Copy them from the doc comments of `classes/std/*.jkc` once those exist in English. | Open |
| T-4 | #26 | track 4 | Modbus `escravo`. | "server" (glossary §7). | Resolved |
| T-5 | #26 | proposal §10, S1 | "Three `out` lines, four methods" for the LED, but `std/led@1` has one `out` member. | Probably three lines of declarations; the author confirms. | Open |
| T-6 | #26 | track 11 | `**Estado no fim:**` without `desta parte`. | The standard "**State at the end of this part:**". | Fixed |
| T-7 | integration | track 4 | "read-back". | "readback" (X1). | Fixed |
| T-8 | #15, #26 | track README, 15 §4 | The heading cited by 15 §4. | "The directory, at the end of the track" in both. | Resolved |

## Counts

| Status | Entries |
|---|---|
| Fixed in the integration pass | 35 |
| Resolved by a translation choice | 32 |
| Open | 58 |
| **Total** | **125** |
