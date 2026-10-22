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
  link/<name>.vectors.toml
  abi/<name>.vectors.toml
  actions/<name>.action.toml
```

| Area | What it covers |
|---|---|
| `arbiter/` | writers, levels, leases, manual writes and TTL (docs/16 §1) |
| `envelope/` | the envelope steps 0–7 and their ordering (docs/01 §7, docs/16 §2) |
| `quality/` | quality propagation in expressions, staleness, initial samples (docs/16 §3, §4; docs/02 §4) |
| `expressions/` | stateful functions, units and number semantics (docs/02 §10, docs/16 §4) |
| `alarms/` | alarm states, trip actions and alarms in permissives (docs/02 §11, ref/01 §5.15) |
| `trace/` | the full per-tick result sequence that the determinism gate compares (docs/16 §7) |
| `link/` | Joker Link test vectors: framing, L2 security, message encodings, the signed OTA manifest (ref/07) |
| `abi/` | Joker Micro C ABI vectors: status codes, enumerations, quality words, input setters (ref/08) |
| `actions/` | M19 device actions end to end: dispatch, driver steps and the decision log (ref/06 §2.10–§2.14, ADR 0058) — its own format, see "Device actions" below |

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

## Test vectors

`link/` and `abi/` hold byte-level vectors (docs/13 §4 task 0c, docs/22 §8). They do not use the timeline schema of the cases. Every number and byte string in them is copied from ref/07 and ref/08, or follows from a rule there. Where the spec prints a value in shortened form (the COSE_Sign1 of ref/07 §9.1), the full bytes were computed with code written independently of `crates/`. They were then checked against everything the spec does print: prefix, suffix, length and signature. Every vector was checked the same way before it was committed: CRC-32C, COBS, HKDF-SHA256, ChaCha20-Poly1305, Ed25519 and deterministic CBOR.

### File format

```toml
schema = 1

[vectors]
kind        = "link.crypto"
id          = "link.crypto"
spec        = ["ref/07 §4.8"]
hosts       = ["linux", "mcu"]
description = "..."

[[vector]]
id   = "key_schedule"
op   = "key_schedule"
spec = ["ref/07 §4.2"]
```

- `schema = 1` is the first key/value pair, and `[vectors]` names the file (`kind`, `id`, `spec`, `hosts`, `description`). `kind` says which `op`s may appear in the file.
- Each `[[vector]]` has an `id` that is unique in the file, an `op` and a `spec` list. The other keys depend on the `op` (table below).
- Byte strings are lowercase hex without separators. Integers are TOML integers. Floats are TOML floats, including `nan` and `inf`.
- `result` is `"accept"` or `"discard"`. A discard may name the `counter` and the registry `code` it increments (ref/07 §3.2, §4.4).
- `abi.input` files carry a fixture in manifest syntax (`[[signals]]`, `[[outputs]]`, as in ref/03 §2.6.2). The runner builds that fixture into a generation for an `extern` device. Each vector starts from a fresh node.

| `kind` | `op` | Keys | What the runner checks |
|---|---|---|---|
| `link.framing` | `crc32c` | `input`, `crc`, `residue` | CRC-32C of `input` is `crc`; `residue` is the reveng residue (the register before the final XOR, after running over data followed by its CRC in little-endian) |
| | `cobs` | `decoded` (and `repeat`), `encoded`, or `encoded_prefix`/`encoded_suffix`/`encoded_length` | COBS encoding and the round trip |
| | `stream_encode` | `addr`, `frame`, `wire` | COBS of `addr`, `frame` and the little-endian CRC-32C, followed by `00`, is `wire` |
| | `stream_decode` | `own_addr`, `wire`, `result`, `counter`, `code`, `addr`, `frame` | the receiver's ordered checks of ref/07 §3.2 |
| `link.crypto` | `key_schedule` | `k_link`, `nonce_d`, `nonce_h`, `sid`, `kid`, `k_hs`, `k_d2h`, `k_h2d`, `k_d2h_next` | the derivations of ref/07 §4.2 (`k_d2h_next` is the key of the next phase) |
| | `kid` | `k_link`, `kid` | |
| | `seal_session`, `seal_handshake` | `key`, `phase`, `sid`, `ctr` or `kid`, `nonce`; `plaintext`, `frame` | the L2 frame of ref/07 §4.1 |
| | `open_session` | `key`, `frame`, `result`, `code`, and on accept `sid`, `ctr`, `plaintext` | tag verification |
| | `first_byte` | `cases = [{ byte, fmt, kind, phase, result, code }]` | the decoding of the first byte, independently of the configured mode |
| | `replay_window` | `steps = [{ ctr, result, code }]` | one direction and one phase, the steps in order from a fresh window |
| `link.message` | `message` (implicit) | `message`, `header` (`type`, `flags`, `seq`, `ack`, `ack_bits`, `rseq`), `body`, `diag`, `abbreviated` | header fields; `body` is exactly one deterministic CBOR item and equals `diag`; with `abbreviated = true` the body holds a placeholder from the spec, so only the encoding is checked |
| `link.ota` | `cose_sign1` | `seed`, `public_key`, `kid`, `board`, `board_id`, `image_zero_len`, `image_digest`, `manifest_diag`, `manifest`, `protected`, `sig_structure_sha256`, `signature`, `cose_sign1` | ref/07 §9.1 end to end |
| `abi.status` | `status` | `macro`, `value`, `code`, `severity` | `jk_status_t` values and `jk_status_str` |
| `abi.enum` | `enum` | `prefix`, `values` | the macro `<prefix><NAME>` has the value |
| `abi.quality` | `quality_word` | `code`, `reason` (name or number), `flags`, `word`, `is_good` | `JK_Q` composes `word`; the decomposition macros give the parts back |
| `abi.input` | `set_input` | `signal`, `setter` (`bool`, `i64`, `u64`, `f64`, `f32`, `buf`), `value`, `quality`, `status`, `read` | the setter returns `status` (the `ts_acq_us` is 0); after one `jk_tick` at 0, `jk_read` gives `read` (for `buf`, `value` is hex; `read.value` is text for `string` and hex for `bytes`); after an error the signal is unchanged (`uncertain(initial)`) |

`quality` is a literal of ref/03 §2.1.3 or a `u32` word (for the private reasons 240–255).

## Device actions

`actions/` cases cover M19 device actions (ref/06 §2.10–§2.14, ADR 0058) end to end: dispatch (serialize per device, dedup by key, gate on `permit`), running the action's compiled steps against its real driver, and the resulting decision-log `action` record. An action is I/O by definition — it runs against a real `Driver` — so it does not fit the core-only timeline schema the other areas share (no device, no driver, no I/O, both `linux` and `mcu` hosts) and is not run by `cargo xtask conformance`. The same reasoning already gave `link/` and `abi/` their own formats; this is a third one, same rationale.

These cases run as a `jokerd`-side test (`crates/jokerd/src/control.rs`'s `conformance_actions` module, `cargo test -p jokerd`), not through the xtask conformance runner. They load the fixture's `joker_toml`/`profile_toml` as a real manifest and profile through `jokerd::load::load` — the exact same path a real node uses — and invoke through a real `Controller::invoke_action`, with every `[[case]]` of a file sharing one `Controller`/decision log in order (so a later case can replay an earlier one's idempotency key, as in the fixture below). The only double is at the driver boundary: ref/06 §2.10 delegates "which registers, in what order" to the driver, and this area is about the dispatch/decision-log mechanics above that boundary, not any one driver's wire protocol — the same level the project's own spy/fake driver tests already use (`crates/jokerd/tests/action_invoke.rs`).

### File format

```toml
schema = 1

[suite]
kind = "action"
id   = "actions.<name>"
spec = ["ref/06 §2.10"]

[fixture]
joker_toml   = "..."
profile_toml = "..."

[[case]]
name   = "..."
spec   = ["..."]
device = "<device name in joker_toml>"
action = "<action name in profile_toml>"
key    = "<idempotency key>"
driver = "succeeds" # or "fails"
after  = "<an earlier case's name>" # optional, documents ordering intent

[case.expect]
state         = "succeeded" # or "failed"
decision_kind = "action"
outcome       = "ok" # or "failed"
replay        = true # optional: this invocation must be a dedup replay, not a fresh run
```

- `schema = 1` is the first key/value pair; `[suite]` names the file (`kind = "action"`, `id`, `spec`).
- `[fixture]` carries a real manifest and a real profile, verbatim, in the same TOML grammar `jokerd::load::load`/`joker-manifest::profile::parse` already accept elsewhere — nothing new to learn here, no comments allowed in them either.
- Every `[[case]]` runs in file order against the fixture's `Controller`. `driver` picks the double's behavior for that one invocation (`succeeds` replies `Ok`; `fails` replies a `Failed` outcome). `after` is documentation only (the runner always runs cases in order); it exists so a reader does not have to infer why a later case's `key` matches an earlier one's.
- `[case.expect].state` is the invocation's outcome as `Controller::invoke_action` reports it (`succeeded`, `failed`, or implicitly `rejected` if the test asserts a `Rejected` variant); `decision_kind`/`outcome` are the matching decision-log `action` record's own `record`/`outcome` fields (ADR 0058 §7); `replay = true` asserts the invocation was served from the dispatcher's dedup cache, not run again.
- Every case carries a `spec` list, same convention as the core suites.
