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
  actions/<suite>.conformance.toml
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
| `actions/` | M19 device and instance actions end to end: role, arguments, permissive, deduplication, the device queue, step and native programs against a register double, long actions and cancellation, and the decision log (ref/09 §5.10, ref/06 §2.10, ADR 0058, ADR 0060, ADR 0066) — suite kind `action`, see "Device actions" below |

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

`actions/` holds suites of kind `action` (ref/03 §2.6.1, ADR 0066 §20–§24). An action is the runtime's dispatch above a driver, so it is I/O and does not fit the core timeline; it runs only on the `linux` host, and `cargo xtask conformance` runs it against the daemon itself: the fixture's manifest is activated as a generation, with its permits, its plan and its decision log, and every device runs its own io task with a **register double** in place of its driver. The steps of a program therefore go through the real interpreter and reach the double's registers, and a native action reaches the double's `invoke`. The QEMU runner skips these files, and the determinism comparison between hosts leaves the `actions.*` suites out.

### File format

```toml
schema = 1

[suite]
kind  = "action"
id    = "actions.<suite>"
spec  = ["ref/09 §5.10"]
hosts = ["linux"]

[fixture]
manifest = """..."""

[fixture.profiles]
"acme/pcs@1" = """..."""

[[double]]
device    = "pcs"
registers = { "30001" = 1 }
on_write  = [{ at = 40310, value = 1, set = { "30010" = 1 } }]
quality   = { "30040" = "bad(comm_failure)" }
fail      = { "40999" = "device_fault" }

[double.native.identify]
result = { serial = 1234 }

[[case]]
id     = "..."
do     = [{ at = "100ms", invoke = "pcs.reset_fault", key = "k1" }]
expect = [{ invocation = "k1", state = "succeeded" }]
```

- `[fixture] manifest` is a manifest in ref/01 syntax, verbatim; `[fixture.profiles]` maps each profile reference it uses to the profile's text (ref/03 §2.2).
- `[[double]]` declares the equipment of one device: `registers` (the value of each address, keyed by the address's `at`), `on_write` (writing `value` at `at` also sets the registers of `set`), `quality` (the quality the double reports for reads of an address, a literal of ref/03 §2.1.3), `fail` (`device_fault`, `timeout` or `comm_failure` for reads and writes of an address), `link = "down"` (every `connect` fails) and `[double.native.<action>]` (`result`, `failure` = `timeout` \| `device` \| `invalid`, `hold` = how long the call runs, checking for cancellation). A device without a `[[double]]` has empty registers.
- `do` runs in order; `at` is the earliest instant, counted from the start of the case in real time:
  - `{ at, invoke = "<target>.<action>", key, ref, args, timeout, reason, wait, role }` invokes as a principal with `role` (default `operator`); `ref` names the invocation for the expectations (default: the key);
  - `{ at, cancel = "<ref>" }` cancels it;
  - `{ at, settle = true }` waits until every invocation ended.
- `expect` is checked once every invocation ended:
  - `{ invocation = "<ref>", state, result, error, code, message, replay, progress }` — the invocation as `/v1/invocations/{id}` shows it (`error` is `error.kind`, `message` a part of `error.message`); `{ invocation = "<ref>", rejected = "<code>" }` for a request error;
  - `{ decision = "<ref>", outcome, kind, code, duplicate, count, instance, device }` — its `action` record (`duplicate = true`: `duplicate_of` names the original invocation);
  - `{ writes = "<device>", equals = [[at, value], …] }` — every write the double received, in order;
  - `{ register = "<device>", at, equals }` — a register at the end of the case.
- Each case starts from a fresh node. A malformed suite is `J0887`; a failed expectation is `J0842`.
- Every case carries a `spec` list, same convention as the core suites.
