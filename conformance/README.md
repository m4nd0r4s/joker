# Conformance cases

The core conformance suites of the Joker. They pin the semantics of the arbiter, the envelope, quality and expressions as data, so that every implementation of the core (Linux and Joker Micro) can be checked against the same timelines and must produce the same bytes (ADR 0020, docs/16 §7 and §8).

## Authorship rule

The expected values in this directory are **written by hand from the specification only** (`docs/`, `spec/`, `wit/`). The author never reads `crates/` and never generates an expectation by running the core: sharing code is sharing bugs (docs/13 §2, AGENTS.md §3). When a case fails against the core, the failure opens a triage issue; the expectation changes only if the specification says otherwise, and then the change cites the paragraph or the ADR that settles it.

Where the specification does not determine an expected value, the case does not guess: it leaves that tick or that predicate out, and the gap is reported for an ADR.

## Layout

```
conformance/
  README.md
  <area>/<suite>.conformance.toml
```

| Area | What it covers |
|---|---|
| `arbiter/` | writers, levels, leases, manual writes and TTL (docs/16 §1) |
| `envelope/` | the envelope steps 0–7 and their ordering (docs/01 §7, docs/16 §2) |
| `quality/` | quality propagation in expressions, staleness, initial samples (docs/16 §3, §4; docs/02 §4) |
| `alarms/` | alarm states and trip actions (docs/02 §11, ref/01 §5.15) |

## File format

Every file follows the schema of ref/03 §2.6 with the core rules of docs/16 §8:

- `schema = 1` is the first key/value pair;
- one `[suite]` with `kind = "core"`, `id` (`<area>.<suite>`), `spec`, `period` and `hosts = ["linux", "mcu"]`;
- the fixture in manifest syntax as lists: `[[signals]]`, `[[outputs]]`, `[[writers]]`, `[[atomic]]`, `[[alarms]]`, `[[calc]]`;
- one or more `[[case]]`, each a timeline of actions (`do`) and expectations (`expect`), with every instant aligned to `period`;
- every case carries a `spec` list with the paragraphs it derives from (`"16 A2"` is docs/16 rule A2, `"01 §7 step 5"` is docs/01 §7, step 5, `"ref/03 §2.6.2"` is the reference). Document numbers are stable; file names may change.

Data files carry no comments (AGENTS.md rule 12). Traceability lives in the `spec` keys.

Each case starts from a clean state: a new generation, every output at `safe` (docs/16 A8), no active writer, alarms `normal` (ref/03 §2.6.1). `equals` on an output is the applied value of the tick, `outcome` the write result, `level` the active arbiter level (`"none"` without one).

## Determinism

All suites declare both hosts. The runner records, for every output of the fixture and every tick a case covers, the result and applied value; the determinism gate compares that full sequence byte for byte between hosts and between runs (docs/16 §7, ref/03 §2.6.4). Expectations check only the ticks and outputs they name.
