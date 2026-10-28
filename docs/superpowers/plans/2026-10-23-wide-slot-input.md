# Wide Slot — Device Input (AUR-419) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `bytes`/`string` device-input signal (an i2c buffer, a Modbus string) actually reaches a wide slot with its real value and quality, and is readable over `GET /v1/values`/`jk read`, instead of always reading `bad(config_error)` as it does on `main` today.

**Architecture:** A new `WideSlot` primitive in `joker-core` (same seqlock algorithm as the existing `Slot`, just 96 bytes instead of 32, for payloads up to 64 bytes) lives in a separate, sparse table in `jokerd::shared::Shared`, sized only to signals actually declared `bytes`/`string`. `jokerd::drivers::run_driver`'s settle loop and `jokerd::wire`'s JSON serialization both branch on the signal's type: scalar types keep going through `Shared::slots` exactly as today; `Bytes`/`String` go through the new `Shared::wide_slots` instead. `Slot` itself, and `joker_core::registry::SignalMeta`, are not touched at all.

**Tech Stack:** Rust, `no_std`+`alloc` dual-target (`joker-core`), `std`+`tokio` (`jokerd`), existing `loom` test infrastructure for the lock-free primitive.

**Reference:** ADR 0062 (`docs/adr/0062-wide-signal-values.md`) is normative; the design note `docs/superpowers/specs/2026-10-23-wide-signal-values-design.md` (§1 and §2) is background, and where it differs from the ADR the ADR wins. Linear issue AUR-419.

## Revision 2026-10-24 (review of AUR-419)

Read this before any task. The plan was written against `main` of 2026-10-23; the fixes of PR #65 (merged 2026-10-24) changed the code it describes, and a review found three problems with the plan itself.

**Corrected premises.**

- *"The write is silently skipped on both success and failure."* No longer true. Since `05ac394` ("write a bad sample when a real driver's read fails") and `7ac3278` ("decode modbus registers and i2c byte buffers"), `run_driver` stores every settled point through `store_settled` → `settled_sample` → `decode_to_bits`, and `mark_bad`/`mark_all` write `bad(<reason>)` when a link goes down or a device cannot start. A wide-typed signal is therefore no longer stuck at `seq = 0`/`uncertain(initial)`: it reads `bad(config_error)` (no value) on every poll, in its **scalar** slot, because `number_to_bits` has no case for `Bytes`/`String`. The gap is the same in kind (a wide value never arrives) but its symptom changed, and the regression test must assert a real value and a real failure quality in the **wide** slot, not merely `seq > 0`.
- *"Exactly mirroring what the scalar path already does for a failed read."* True now (it was not when the plan was written): the scalar failure path writes `bad`. The wide path must mirror `settled_sample` (a `bad` status keeps its reason, a value that does not convert is `bad(config_error)`) **and** `mark_all` (link down, device that cannot start).
- *"Numeric i2c reads need the WideSlot."* No: since `7ac3278` an i2c point typed `u16`/`i16`/… decodes into its scalar signal. Only points declared `string` or `bytes` need this plan. AUR-401's bench smoke test must declare the point `type = "bytes"` to exercise it.
- *Truncation.* ADR 0062 decision 3 replaces the design's silent truncation: a write of more than 64 B stores zero bytes and `bad(config_error)`. Task 2 follows the ADR.
- *Decoding.* ADR 0062 decision 5 also covers Modbus `string`/`bytes` points (`OwnedRaw::Regs`), `encoding = "ascii"`/`"utf8"` validation and the 64 B limit. Task 4 implements all of it, not only `Bytes`/`Text`.
- *Staleness.* `max_age` applies to wide signals (ADR 0062 decision 13); Task 5 reuses `sample`'s staleness grading instead of skipping it.

**File pointers on `main` at `512e1ce`.** `slot.rs` `lo`/`hi`/`join`: lines 166–176 (unchanged). `Shared::slots` is built at `crates/jokerd/src/shared.rs:384-385`; `Plan.signals` ("in path order") at `crates/jokerd/src/plan.rs:160-161`; `SignalPlan.ty` at `plan.rs:90`, and `SignalPlan` now also has `enum_name: Option<String>` (every struct literal below needs `enum_name: None`). The settle write is `store_settled` at `crates/jokerd/src/drivers.rs:306-311`, the failure write `mark_all` at `drivers.rs:465-471`, and `PointTarget` (`drivers.rs:316`) now carries `decode: Decode`; build targets with `jokerd::drivers::point_targets(&plan, i)` instead of struct literals. `drivers.rs` imports `OwnedRaw`, `SampleBuffer`, `Settled` and `settle` at line 19. `GET /v1/values` is `fn values` at `crates/jokerd/src/api.rs:873-893`; `GET /v1/signals?sample=true` reads samples at `api.rs:1652-1662` and needs the same wide branch.

**Workflow.** This repository uses jj, and nothing goes to `main` directly. Work in your own workspace (`jj workspace add ../joker-aur-419 -r main@origin`), check `jj config get user.email` before the first commit, make one atomic commit per task with `jj commit -m "type(scope): summary" -m "why"` (no AI attribution), keep the code free of comments (AGENTS.md invariant 12), then push a branch (`jj bookmark create aur-419-wide-slot-input -r @-` and `jj git push --bookmark aur-419-wide-slot-input`) and open a pull request with base `main` that names AUR-419. Move AUR-419 to In Review; the owner merges.

---

### Task 1: Make `Slot`'s word-packing helpers reusable

**Files:**
- Modify: `crates/joker-core/src/slot.rs:166-176`

`WideSlot` (Task 2) needs the exact same `lo`/`hi`/`join` 32↔64-bit helpers `Slot` already has. Reuse them instead of duplicating.

- [ ] **Step 1: Widen visibility**

In `crates/joker-core/src/slot.rs`, change:

```rust
const fn lo(word: u64) -> u32 {
    word as u32
}

const fn hi(word: u64) -> u32 {
    (word >> 32) as u32
}

const fn join(lo: u32, hi: u32) -> u64 {
    (lo as u64) | ((hi as u64) << 32)
}
```

to:

```rust
pub(crate) const fn lo(word: u64) -> u32 {
    word as u32
}

pub(crate) const fn hi(word: u64) -> u32 {
    (word >> 32) as u32
}

pub(crate) const fn join(lo: u32, hi: u32) -> u64 {
    (lo as u64) | ((hi as u64) << 32)
}
```

- [ ] **Step 2: Verify nothing else broke**

Run: `cargo test -p joker-core --lib slot::`
Expected: existing `slot::tests::*` all still pass (this is a pure visibility widening, no behavior change).

- [ ] **Step 3: Commit**

```bash
jj commit -m "refactor(joker-core): widen Slot's word-packing helpers to pub(crate)" -m "Needed by the new WideSlot (same packing scheme, bigger payload) so it
doesn't duplicate lo/hi/join."
```

---

### Task 2: `WideSlot` primitive in `joker-core`

**Files:**
- Create: `crates/joker-core/src/wide_slot.rs`
- Modify: `crates/joker-core/src/lib.rs` (add `pub mod wide_slot;` and re-export)

**Background:** `joker_core::slot::Slot` is the existing 32-byte seqlock last-value slot (`crates/joker-core/src/slot.rs:1-121`): a `version: AtomicU32` that's odd while a write is in progress, plus `quality`/`value_lo`/`value_hi`/`ts_src_lo`/`ts_src_hi`/`ts_acq_lo`/`ts_acq_hi`, each its own `AtomicU32` (no `unsafe`, portable to MCUs without 64-bit atomics). `WideSlot` is the same algorithm, sized for up to 64 bytes of payload instead of one `u64`.

- [ ] **Step 1: Write the failing tests**

Create `crates/joker-core/src/wide_slot.rs`:

```rust
//! The wide (<=64 byte) last-value slot with its seqlock (docs/02-signal-model.md
//! §2, docs/04-sdk.md's `Internal<T>`).
//!
//! Same seqlock algorithm as [`crate::slot::Slot`], deliberately not a
//! bigger `Slot`: every scalar signal keeps paying zero cost for a
//! feature only `bytes`/`string` signals use.

use crate::quality::{Quality, QualityCode, QualityReason};
use crate::slot::Collision;
use crate::slot::{hi, join, lo};
use crate::sync::{AtomicU32, Ordering, fence};
use crate::value::NO_TS_SRC;

/// Maximum payload bytes a [`WideSlot`] holds (docs/02-signal-model.md §2:
/// `bytes`/`string` are each <=64 bytes).
pub const WIDE_SLOT_CAPACITY: usize = 64;
const PAYLOAD_WORDS: usize = WIDE_SLOT_CAPACITY / 4;

/// A last-value slot for a `bytes`/`string` signal, up to
/// [`WIDE_SLOT_CAPACITY`] bytes: single writer, any number of wait-free
/// readers, same seqlock shape as [`crate::slot::Slot`].
#[derive(Debug)]
#[repr(C, align(32))]
pub struct WideSlot {
    version: AtomicU32,
    quality: AtomicU32,
    ts_src_lo: AtomicU32,
    ts_src_hi: AtomicU32,
    ts_acq_lo: AtomicU32,
    ts_acq_hi: AtomicU32,
    len: AtomicU32,
    payload: [AtomicU32; PAYLOAD_WORDS],
}

/// A read sample of a [`WideSlot`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WideSample {
    bytes: [u8; WIDE_SLOT_CAPACITY],
    len: u8,
    quality: Quality,
    ts_src: i64,
    ts_acq: u64,
    seq: u64,
}

impl WideSample {
    /// The sample of a slot that never received a value:
    /// `uncertain(initial)`, zero bytes, sequence 0.
    pub const INITIAL: Self = Self {
        bytes: [0; WIDE_SLOT_CAPACITY],
        len: 0,
        quality: Quality::INITIAL,
        ts_src: NO_TS_SRC,
        ts_acq: 0,
        seq: 0,
    };

    /// The bytes actually written (`len` of them, never more).
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The quality.
    pub const fn quality(&self) -> Quality {
        self.quality
    }

    /// The raw `ts_src` field ([`NO_TS_SRC`] when absent).
    pub const fn ts_src_raw(&self) -> i64 {
        self.ts_src
    }

    /// Monotonic ns of acquisition.
    pub const fn ts_acq(&self) -> u64 {
        self.ts_acq
    }

    /// The slot's sequence number at the time of this read (0 means
    /// never written).
    pub const fn seq(&self) -> u64 {
        self.seq
    }
}

impl Default for WideSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl WideSlot {
    /// An empty slot: sequence 0, zero bytes, `uncertain(initial)`.
    pub fn new() -> Self {
        Self {
            version: AtomicU32::new(0),
            quality: AtomicU32::new(Quality::INITIAL.to_bits()),
            ts_src_lo: AtomicU32::new(lo(NO_TS_SRC as u64)),
            ts_src_hi: AtomicU32::new(hi(NO_TS_SRC as u64)),
            ts_acq_lo: AtomicU32::new(0),
            ts_acq_hi: AtomicU32::new(0),
            len: AtomicU32::new(0),
            payload: [const { AtomicU32::new(0) }; PAYLOAD_WORDS],
        }
    }

    /// Stores a new sample and returns its 31-bit sequence number.
    /// `bytes` longer than [`WIDE_SLOT_CAPACITY`] is never truncated: it
    /// stores zero bytes with `bad(config_error)` (ADR 0062, decision 3).
    /// Producers check the length first; this is a last line of defence.
    ///
    /// Only the slot's single writer may call this (docs/01 §3.2's own
    /// rule for [`crate::slot::Slot::write`] applies here identically).
    pub fn write(&self, bytes: &[u8], quality: Quality, ts_src: i64, ts_acq: u64) -> u32 {
        let (bytes, quality) = if bytes.len() > WIDE_SLOT_CAPACITY {
            (&[][..], Quality::new(QualityCode::Bad, QualityReason::ConfigError))
        } else {
            (bytes, quality)
        };
        let len = bytes.len();
        let version = self.version.load(Ordering::Relaxed);
        let busy = version.wrapping_add(1);
        self.version.store(busy, Ordering::Relaxed);
        fence(Ordering::Release);
        self.quality.store(quality.to_bits(), Ordering::Relaxed);
        self.len.store(len as u32, Ordering::Relaxed);
        for (i, word) in self.payload.iter().enumerate() {
            let start = i * 4;
            let mut buf = [0u8; 4];
            if start < len {
                let n = (len - start).min(4);
                buf[..n].copy_from_slice(&bytes[start..start + n]);
            }
            word.store(u32::from_le_bytes(buf), Ordering::Relaxed);
        }
        self.ts_src_lo.store(lo(ts_src as u64), Ordering::Relaxed);
        self.ts_src_hi.store(hi(ts_src as u64), Ordering::Relaxed);
        self.ts_acq_lo.store(lo(ts_acq), Ordering::Relaxed);
        self.ts_acq_hi.store(hi(ts_acq), Ordering::Relaxed);
        let done = busy.wrapping_add(1);
        self.version.store(done, Ordering::Release);
        done >> 1
    }

    /// One read attempt. Returns [`Collision`] if a write was in progress.
    pub fn try_read(&self) -> Result<WideSample, Collision> {
        let before = self.version.load(Ordering::Acquire);
        if before & 1 == 1 {
            return Err(Collision);
        }
        let quality = self.quality.load(Ordering::Relaxed);
        let len = self.len.load(Ordering::Relaxed);
        let mut bytes = [0u8; WIDE_SLOT_CAPACITY];
        for (i, word) in self.payload.iter().enumerate() {
            let w = word.load(Ordering::Relaxed).to_le_bytes();
            let start = i * 4;
            bytes[start..start + 4].copy_from_slice(&w);
        }
        let ts_src_lo = self.ts_src_lo.load(Ordering::Relaxed);
        let ts_src_hi = self.ts_src_hi.load(Ordering::Relaxed);
        let ts_acq_lo = self.ts_acq_lo.load(Ordering::Relaxed);
        let ts_acq_hi = self.ts_acq_hi.load(Ordering::Relaxed);
        fence(Ordering::Acquire);
        let after = self.version.load(Ordering::Relaxed);
        if before != after {
            return Err(Collision);
        }
        Ok(WideSample {
            bytes,
            len: len.min(WIDE_SLOT_CAPACITY as u32) as u8,
            quality: Quality::from_bits(quality),
            ts_src: join(ts_src_lo, ts_src_hi) as i64,
            ts_acq: join(ts_acq_lo, ts_acq_hi),
            seq: u64::from(before >> 1),
        })
    }

    /// Reads with at most `retries` extra attempts after the first (same
    /// give-up-on-persistent-collision rule as [`crate::slot::Slot::read`]).
    pub fn read(&self, retries: u32) -> Result<WideSample, Collision> {
        let mut attempt = 0;
        loop {
            match self.try_read() {
                Ok(sample) => return Ok(sample),
                Err(Collision) if attempt < retries => attempt += 1,
                Err(Collision) => return Err(Collision),
            }
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use crate::quality::{QualityCode, QualityReason};

    extern crate std;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn wide_slot_is_96_bytes_and_aligned() {
        assert_eq!(core::mem::size_of::<WideSlot>(), 96);
        assert_eq!(core::mem::align_of::<WideSlot>(), 32);
    }

    #[test]
    fn empty_wide_slot_reads_initial() {
        assert_eq!(WideSlot::new().try_read(), Ok(WideSample::INITIAL));
    }

    #[test]
    fn write_then_read() {
        let slot = WideSlot::new();
        let q = Quality::new(QualityCode::Bad, QualityReason::CommFailure);
        assert_eq!(slot.write(b"hi", q, -5, 1 << 40), 1);
        assert_eq!(slot.write(b"hello", Quality::GOOD, NO_TS_SRC, 3), 2);
        let s = slot.read(0).unwrap();
        assert_eq!(s.as_slice(), b"hello");
        assert_eq!(s.quality(), Quality::GOOD);
        assert_eq!(s.ts_src_raw(), NO_TS_SRC);
        assert_eq!(s.ts_acq(), 3);
        assert_eq!(s.seq(), 2);
    }

    #[test]
    fn a_payload_longer_than_64_bytes_is_bad_config_error_not_truncated() {
        let slot = WideSlot::new();
        let too_long = [b'x'; 100];
        slot.write(&too_long, Quality::GOOD, NO_TS_SRC, 0);
        let s = slot.read(0).unwrap();
        assert!(s.as_slice().is_empty());
        assert_eq!(s.quality().code(), QualityCode::Bad);
        assert_eq!(s.quality().reason(), QualityReason::ConfigError);
        assert_eq!(s.seq(), 1);
    }

    #[test]
    fn concurrent_readers_never_see_torn_samples() {
        let slot = Arc::new(WideSlot::new());
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let slot = Arc::clone(&slot);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                let mut k: u8 = 1;
                while !stop.load(core::sync::atomic::Ordering::Relaxed) {
                    let buf = [k; WIDE_SLOT_CAPACITY];
                    slot.write(&buf, Quality::GOOD, 0, u64::from(k));
                    k = k.wrapping_add(1).max(1);
                }
            })
        };
        let mut ok = 0u32;
        for _ in 0..200_000 {
            if let Ok(s) = slot.read(4) {
                if s.seq() == 0 {
                    assert_eq!(s, WideSample::INITIAL);
                    continue;
                }
                let bytes = s.as_slice();
                let k = bytes[0];
                assert!(bytes.iter().all(|&b| b == k), "torn wide sample");
                assert_eq!(s.ts_acq(), u64::from(k));
                ok += 1;
            }
        }
        stop.store(true, core::sync::atomic::Ordering::Relaxed);
        assert!(writer.join().is_ok());
        assert!(ok > 0);
    }
}

#[cfg(all(test, loom))]
mod loom_tests {
    use super::*;
    use loom::sync::Arc;
    use loom::thread;

    #[test]
    fn reader_sees_whole_samples_only() {
        loom::model(|| {
            let slot = Arc::new(WideSlot::new());
            let writer = {
                let slot = Arc::clone(&slot);
                thread::spawn(move || {
                    for k in 1..=2u8 {
                        let buf = [k; WIDE_SLOT_CAPACITY];
                        slot.write(&buf, Quality::GOOD, 0, u64::from(k));
                    }
                })
            };
            if let Ok(s) = slot.try_read() {
                if s.seq() == 0 {
                    assert_eq!(s, WideSample::INITIAL);
                } else {
                    let bytes = s.as_slice();
                    let k = bytes[0];
                    assert!(bytes.iter().all(|&b| b == k));
                    assert_eq!(s.ts_acq(), u64::from(k));
                }
            }
            assert!(writer.join().is_ok());
        });
    }
}
```

- [ ] **Step 2: Wire the module in, and re-export, in `crates/joker-core/src/lib.rs`**

Find the line `pub mod slot;` (near the other `pub mod` declarations) and add directly after it:

```rust
pub mod wide_slot;
```

Find the line `pub use slot::{Collision, SeqExtender, Slot};` and add directly after it:

```rust
pub use wide_slot::{WIDE_SLOT_CAPACITY, WideSample, WideSlot};
```

- [ ] **Step 3: Run the tests and verify they fail first (RED), then pass (GREEN)**

Before adding the module to `lib.rs` (Step 2), running the tests fails to compile (`wide_slot` module doesn't exist, confirming RED). After Step 2:

Run: `cargo test -p joker-core --lib wide_slot::`
Expected: `wide_slot_is_96_bytes_and_aligned`, `empty_wide_slot_reads_initial`, `write_then_read`, `a_payload_longer_than_64_bytes_is_bad_config_error_not_truncated`, `concurrent_readers_never_see_torn_samples` all `ok`.

- [ ] **Step 4: Run the loom test**

This repo's own CI (`.github/workflows/nightly.yml`, job `core-loom`) runs `joker-core`'s loom tests as: `RUSTFLAGS="--cfg loom" LOOM_MAX_PREEMPTIONS=2 cargo test --release -p joker-core --lib loom`. Run the same command, scoped to the new test:

Run: `RUSTFLAGS="--cfg loom" LOOM_MAX_PREEMPTIONS=2 cargo test --release -p joker-core --lib wide_slot::loom_tests`
Expected: `reader_sees_whole_samples_only` passes.

**As done (2026-10-28).** The first implementation (`dc2a8d8`) dropped this step: an exhaustive loom model over sixteen payload words did not finish, and the threaded test of Step 3 was kept as the only check. That left two gaps: the algorithm was argued to be `Slot`'s rather than checked, and the threaded test wrote the same byte into every word at a fixed length, so it could not see a tear between words of different writes, a length that disagrees with its payload, or a quality and timestamp from another write. The follow-up closed both:

- Under `--cfg loom` only, `WIDE_SLOT_CAPACITY` is 8 (two payload words) instead of 64. The seqlock code is unchanged; only the number of words it loops over shrinks, so the model finishes (about 50 s with `Slot`'s two models, `LOOM_MAX_PREEMPTIONS=2`).
- `wide_slot::loom_tests` has `reader_sees_whole_samples_only` (each successful read is one whole write: bytes, length, quality, `ts_src`, `ts_acq`, all tied to its sequence number, with two writes of different lengths) and `two_reads_are_monotonic`.
- The threaded test is now `concurrent_readers_see_only_whole_samples`: every write `k` has its own length, a distinct byte per position, its own quality and timestamps, all derived from `k`, and `k` is read back from the sample's `seq`; it checks every field, that `seq` never goes backwards, and that at least 2,000 distinct versions were read while the writer ran.
- Mutation check: removing the version recheck at the end of `try_read` fails both the loom model and the threaded test.

- [ ] **Step 5: Run a mutation check**

Temporarily change the guard `bytes.len() > WIDE_SLOT_CAPACITY` to `false` (so an oversized write is stored) and re-run `a_payload_longer_than_64_bytes_is_bad_config_error_not_truncated` — it must fail or panic on the slice bound (this proves the test exercises the guard). Revert the change.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p joker-core --all-targets -- -D warnings`
Expected: clean (no new warnings; `joker-core`'s `[lints.clippy]` denies `unwrap_used`/`expect_used`/`panic` outside `#[cfg(test)]`, which the code above never uses outside the test module).

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(joker-core): WideSlot, a <=64-byte seqlock last-value slot" -m "Same algorithm as Slot (odd version = write in progress, every field
AtomicU32, no unsafe), sized for bytes/string signals up to 64 bytes
(docs/02-signal-model.md §2). Slot itself is untouched - zero risk to
the existing scalar path, which is used by every signal regardless of
type."
```

---

### Task 3: `wide_slots` table and `wide_slot()` accessor on `jokerd::shared::Shared`

**Files:**
- Modify: `crates/jokerd/src/shared.rs`
- Test: `crates/jokerd/tests/shared.rs` (create if it doesn't exist; check first with `ls crates/jokerd/tests/shared.rs`)

**Background:** `jokerd::plan::Plan.signals: Vec<SignalPlan>` is already "in path order" (`crates/jokerd/src/plan.rs:160-161`'s own doc comment), and `Shared::new` already assumes `plan.signals[i]` corresponds to `SignalId(i)` (`crates/jokerd/src/shared.rs:384-385`: `let slots: Box<[Slot]> = (0..plan.signals.len()).map(|_| Slot::new()).collect();`, and elsewhere `shared.slots.get(target.signal.index())`). `SignalPlan.ty: ValueType` is already there (`crates/jokerd/src/plan.rs:90`) — no `joker-core` registry changes are needed to build the wide-signal index; it's a pure `jokerd`-side derived table computed once in `Shared::new`, from data `jokerd` already has.

- [ ] **Step 1: Check whether `crates/jokerd/tests/shared.rs` already exists**

Run: `ls crates/jokerd/tests/shared.rs`

If it exists, read it first and add the new test to its existing `mod`/file structure, following its own conventions. If it does not exist (most likely — `Shared` is usually tested indirectly through `crates/jokerd/tests/drivers.rs` and similar), create it fresh in Step 2.

- [ ] **Step 2: Write the failing test**

Create (or append to) `crates/jokerd/tests/shared.rs`:

```rust
//! `jokerd::shared::Shared`'s wide-slot table.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use joker_manifest::OsFileSystem;

use jokerd::load::load;
use jokerd::native::NativeTasks;
use jokerd::shared::Shared;

const JOKER_TOML: &str = r#"
[project]
name  = "shared-wide-test"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "50ms" }
"#;

#[test]
fn a_wide_signal_gets_a_wide_slot_and_a_scalar_signal_does_not() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());

    let shared = Arc::new(Shared::new(&loaded.plan));
    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    assert!(
        shared.wide_slot(temp_id).is_some(),
        "a bytes signal must resolve to a WideSlot"
    );

    let out_of_range = joker_core::SignalId(temp_id.0 + 1);
    assert!(
        shared.wide_slot(out_of_range).is_none(),
        "an id past the end of the plan resolves to no wide slot"
    );
}

#[test]
fn a_wide_slot_starts_uninitialized_and_advances_after_a_write() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), JOKER_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    let shared = Shared::new(&loaded.plan);
    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    let slot = shared.wide_slot(temp_id).unwrap();
    assert_eq!(slot.read(0).unwrap().seq(), 0);
    slot.write(b"hi", joker_core::Quality::GOOD, joker_core::value::NO_TS_SRC, 1);
    assert_eq!(slot.read(0).unwrap().seq(), 1);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p jokerd --test shared`
Expected: FAIL to compile — `Shared::wide_slot` does not exist yet.

- [ ] **Step 4: Implement `wide_slots`/`wide_index` on `Shared`**

In `crates/jokerd/src/shared.rs`, change the `use` block at the top (currently):

```rust
use joker_core::{Quality, SignalId, Slot, ValueType};
```

to:

```rust
use joker_core::{Quality, SignalId, Slot, ValueType, WideSlot};
```

In the `Shared` struct (right after the `pub slots: Box<[Slot]>,` field), add:

```rust
    /// One wide slot per `bytes`/`string` signal, indexed densely among
    /// only those signals (not by `SignalId` directly — see
    /// [`Shared::wide_index`]). Empty when the manifest declares none.
    pub wide_slots: Box<[WideSlot]>,
    /// `SignalId` to its index into [`Shared::wide_slots`], or `None` for
    /// a scalar signal. Same length and order as [`Shared::slots`], built
    /// once in [`Shared::new`].
    wide_index: Box<[Option<u32>]>,
```

Right after the existing `impl Shared { pub fn new(plan: &Plan) -> Self {` line and its first statement (`let slots: Box<[Slot]> = ...`), add:

```rust
        let mut wide_index: Vec<Option<u32>> = Vec::with_capacity(plan.signals.len());
        let mut wide_count: u32 = 0;
        for s in &plan.signals {
            if matches!(s.ty, ValueType::Bytes | ValueType::String | ValueType::Record) {
                wide_index.push(Some(wide_count));
                wide_count += 1;
            } else {
                wide_index.push(None);
            }
        }
        let wide_slots: Box<[WideSlot]> = (0..wide_count).map(|_| WideSlot::new()).collect();
```

In the `Self { ... }` construction inside `new`, add the two new fields (right after `slots,`):

```rust
            slots,
            wide_slots,
            wide_index: wide_index.into_boxed_slice(),
```

After the `impl Shared` block's existing `pub fn new(...)` method, add a new method:

```rust
    /// The wide slot of `signal`, or `None` if it is not a `bytes`/
    /// `string` signal.
    pub fn wide_slot(&self, signal: SignalId) -> Option<&WideSlot> {
        let index = self.wide_index.get(signal.index()).copied().flatten()?;
        self.wide_slots.get(index as usize)
    }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p jokerd --test shared`
Expected: both tests `ok`.

- [ ] **Step 6: Run the rest of `jokerd`'s test suite to check nothing else broke**

Run: `cargo test -p jokerd --lib`
Expected: all existing tests still pass (this task only adds fields/methods; nothing existing changes shape).

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p jokerd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
jj commit -m "feat(jokerd): Shared gains a sparse WideSlot table for bytes/string signals" -m "Built once in Shared::new from plan.signals (already in SignalId
order, already carries ty: ValueType) - no joker-core registry changes
needed. Sized only to signals actually declared bytes/string; zero for
a manifest that declares none."
```

---

### Task 4: Wire the settle and failure paths of `jokerd::drivers`

**Files:**
- Modify: `crates/jokerd/src/drivers.rs`
- Test: `crates/jokerd/tests/drivers.rs`

**Background (on `main` at `512e1ce`):** every settled point goes through

```rust
fn store_settled(shared: &Shared, target: &PointTarget, settled: &Settled, now: u64) {
    let (bits, quality) = settled_sample(target, settled);
    if let Some(slot) = shared.slots.get(target.signal.index()) {
        slot.write(bits, quality, NO_TS_SRC, settled.ts_hw.unwrap_or(now));
    }
}
```

(`crates/jokerd/src/drivers.rs:306-311`), and a link that goes down or a device that cannot start goes through `mark_all` (`drivers.rs:465-471`), which writes `bad(<reason>)` into the scalar slot of every target. For a `bytes`/`string` signal `settled_sample` calls `decode_to_bits`, which ends in `number_to_bits` with no case for wide types, so the signal reads `bad(config_error)` in its scalar slot on every poll, and the wide slot never receives anything. Both paths must write the wide slot instead, following ADR 0062 decisions 5 and 6: the decoded bytes and the driver's quality on success; zero bytes and the real `bad` quality on failure; zero bytes and `bad(config_error)` for a value that does not decode (wrong raw kind, over 64 B, a `string` that fails its `encoding`).

- [ ] **Step 1: Write the failing test**

Append to `crates/jokerd/tests/drivers.rs`. Follow the current shape of `a_read_that_starts_failing_turns_the_slot_bad_instead_of_keeping_the_last_value` in the same file for the `run_driver` call (it returns `()`; `host.run(...)` is unwrapped) and build the targets with `point_targets`, which fills `decode`. Check that the i2c manifest below still loads without errors (the test asserts it) and adjust its keys to ref/01 and ref/03 if it does not:

```rust
#[test]
fn a_wide_read_reaches_its_wide_slot_on_success_and_on_failure() {
    use joker_driver_i2c::{I2cDriver, I2cPlanner};
    use joker_hal::testing::MemoryI2c;
    use joker_hal::{I2cBus, Link};

    const I2C_TOML: &str = r#"
[project]
name  = "drivers-i2c-test"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "20ms" }
"#;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("joker.toml"), I2C_TOML).unwrap();
    let loaded = load(&OsFileSystem, &dir.path().join("joker.toml"), &NativeTasks)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!loaded.report.has_errors(), "{}", loaded.report.render());
    let device_plan = loaded.plan.devices[0].clone();

    let shared = Arc::new(Shared::new(&loaded.plan));
    let owned = jokerd::bridge::owned_request(&device_plan);
    let views = owned.views();
    let plan = I2cPlanner::plan(&views.request()).unwrap();

    let (bus, handle) = MemoryI2c::new();
    handle.preload(0x40, &[0xAB, 0xCD]);
    let link = Link::I2c(I2cBus::new(bus));
    let driver = I2cDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        link,
    )
    .unwrap();

    let temp_id = loaded.plan.id("sensor.temp").unwrap();
    let targets = jokerd::drivers::point_targets(&loaded.plan, 0);

    let host = Arc::new(VirtualHost::new(0));
    let stop_at = 100_000_000u64;
    let host_for_stop = Arc::clone(&host);
    host.run(run_driver(
        driver,
        &*host,
        views.request(),
        plan.view(),
        2_000_000_000,
        &targets,
        &shared,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || host_for_stop.mono() >= stop_at,
    ))
    .unwrap();

    let sample = shared.wide_slot(temp_id).unwrap().read(8).unwrap();
    assert!(sample.seq() > 0, "a successful wide read must advance past seq 0");
    assert_eq!(sample.as_slice(), &[0xAB, 0xCD]);
    assert!(sample.quality().is_good());

    let host2 = Arc::new(VirtualHost::new(0));
    let (bus2, handle2) = MemoryI2c::new();
    handle2.preload(0x40, &[0xAB, 0xCD]);
    handle2.deny(0x40);
    let link2 = Link::I2c(I2cBus::new(bus2));
    let driver2 = I2cDriver::open(
        &joker_hal::OpenArgs::new(views.request(), plan.view()),
        link2,
    )
    .unwrap();
    let shared2 = Arc::new(Shared::new(&loaded.plan));
    let host2_for_stop = Arc::clone(&host2);
    host2.run(run_driver(
        driver2,
        &*host2,
        views.request(),
        plan.view(),
        2_000_000_000,
        &targets,
        &shared2,
        0,
        None,
        tokio::sync::mpsc::channel(1).1,
        move || host2_for_stop.mono() >= stop_at,
    ))
    .unwrap();
    let failed_sample = shared2.wide_slot(temp_id).unwrap().read(8).unwrap();
    assert!(
        failed_sample.seq() > 0,
        "a FAILED wide read must also advance past seq 0, with a bad quality"
    );
    assert_eq!(failed_sample.quality().code(), joker_core::QualityCode::Bad);
    assert!(failed_sample.as_slice().is_empty(), "a bad wide sample carries no bytes");
    let scalar = shared2.slots[temp_id.index()].read(8).unwrap();
    assert_eq!(scalar.seq(), 0, "the scalar slot of a wide signal is never written");
}
```

Add a second test, `a_wide_value_that_does_not_decode_is_bad_config_error`, that calls `jokerd::drivers` with a settled `OwnedRaw::Bytes` of 65 bytes, and one with `OwnedRaw::Text` holding a non-ASCII character for a `string` point whose `encoding` is the default `ascii`, and asserts zero bytes and `bad(config_error)` in the wide slot. Add a third, `a_wide_signal_turns_bad_when_its_link_goes_down`, that calls `mark_bad` on wide targets and asserts the wide slot reads `bad(<reason>)`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p jokerd --test drivers a_wide_read_reaches_its_wide_slot_on_success_and_on_failure`
Expected: FAIL — `Shared::wide_slot` exists (Task 3), but the wide sample never advances past `seq == 0`, because `store_settled` writes `bad(config_error)` into the scalar slot instead.

- [ ] **Step 3: Implement the wide branch of `store_settled` and `mark_all`**

In `crates/jokerd/src/drivers.rs`:

1. Add `pub fn decode_to_wide(value: &OwnedRaw, decode: &Decode, ty: ValueType) -> Option<Vec<u8>>` next to `decode_to_bits` (ADR 0062 decision 5): `OwnedRaw::Bytes(b)` as is; `OwnedRaw::Text(t)` as its UTF-8 bytes; `OwnedRaw::Regs(words)` as two bytes per word, big-endian, swapped when `decode.bytes_little`, with no `word_order`; anything else `None`. Then `None` when the result is longer than 64 B, or when `ty` is `String` and the bytes are not valid UTF-8 or, with the point's `encoding = "ascii"` (the default), contain a byte `>= 0x80`. `Decode` does not carry `encoding` yet: add it where `PointTarget` is built from the point's profile entry (`point_targets`). This runs in the io task, not in the cycle, so a `Vec` is allowed; if you prefer, return a `([u8; 64], usize)` instead.
2. Add `pub fn settled_wide(target: &PointTarget, settled: &Settled) -> (Vec<u8>, Quality)`, the wide twin of `settled_sample`: a `bad` status gives no bytes and keeps its reason; a value that `decode_to_wide` refuses gives no bytes and `bad(config_error)`; otherwise the bytes and the status.
3. In `store_settled`, before the scalar path:

```rust
    if is_wide(target.ty) {
        let (bytes, quality) = settled_wide(target, settled);
        if let Some(wide) = shared.wide_slot(target.signal) {
            wide.write(&bytes, quality, NO_TS_SRC, settled.ts_hw.unwrap_or(now));
        }
        return;
    }
```

4. In `mark_all`, for a wide target, write `wide.write(&[], quality, NO_TS_SRC, now)` into its wide slot and skip the scalar slot.
5. `is_wide(ty)` is `matches!(ty, ValueType::Bytes | ValueType::String | ValueType::Record)`; put it next to `Shared::wide_slot` so Tasks 3, 4 and 6 share it.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p jokerd --test drivers a_wide_read_reaches_its_wide_slot_on_success_and_on_failure`
Expected: `ok`.

- [ ] **Step 5: Run a mutation check**

Temporarily remove the `return;` after the wide write in `store_settled` (so a wide point also falls through to the scalar path) and re-run the test: the assertion that the scalar slot of a wide signal is never written must fail. Revert the mutation once confirmed.

- [ ] **Step 6: Run the rest of `jokerd`'s driver tests**

Run: `cargo test -p jokerd --test drivers`
Expected: the existing tests (`run_driver_reads_and_writes_through_the_real_commit_channel`, `a_read_that_starts_failing_turns_the_slot_bad_instead_of_keeping_the_last_value`, `a_value_that_does_not_convert_to_the_signal_type_is_bad_config_error`), `crates/jokerd/tests/raw_bits.rs` and the new ones all pass — the scalar path is unchanged for non-wide signals.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p jokerd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Update the module documentation**

The module docstring of `crates/jokerd/src/drivers.rs` describes how `Regs` and `Bytes` are decoded into scalar signals (`decode_to_bits`). Add that `string`/`bytes` signals are decoded by `decode_to_wide` into their wide slot (ADR 0062), and that their scalar slot is never written.

- [ ] **Step 9: Commit**

```bash
jj commit -m "fix(jokerd): a bytes/string driver read reaches its wide slot" -m "A string or bytes signal read bad(config_error) on every poll, in its
scalar slot, because decode_to_bits has no case for wide types: the
real i2c adapter on the bench could never deliver a buffer. Settled
wide points are now decoded per ADR 0062 (bytes, text and Modbus
registers, the 64 B limit, the ascii/utf8 encoding) and written to the
signal's wide slot, and a failed read or a link that goes down writes
the real bad quality there, as the scalar path does."
```

---

### Task 5: `jokerd::wire` serialization for wide values

**Files:**
- Modify: `crates/jokerd/src/wire.rs`

**Background:** `wire::value(signal: &SignalPlan, bits: u64) -> Value` (`crates/jokerd/src/wire.rs:42-71`) converts a scalar signal's `u64` bits to its JSON form. `wire::sample(signal: &SignalPlan, s: &Sample, now_mono_ns: u64, utc_origin_ns: i64) -> Value` (`crates/jokerd/src/wire.rs:105-141`) builds the full JSON object (`path`/`value`/`unit`/`quality`/`reason`/`ts_acq`/`seq`/`age`) for `GET /v1/values`. Both need a wide counterpart. Per `ref/00-conventions.md` §4.1 (already normative, not a new decision): `bytes` → base64, `string` → plain JSON string.

- [ ] **Step 1: Check what base64 crate (if any) is already a dependency**

Run: `grep -n "base64" crates/jokerd/Cargo.toml Cargo.toml`

On `main` at `512e1ce` no crate depends on `base64`. Adding it is a new dependency: it must pass `cargo deny` and `cargo vet` (ADR 0055); record the audit or exemption in `supply-chain/` in the same commit. A 30-line encoder/decoder in `wire.rs` (standard alphabet, padded, RFC 4648 §4) is an acceptable alternative that avoids the dependency.

- [ ] **Step 2: Add `base64` as a dependency if Step 1 found none**

If needed, add to `crates/jokerd/Cargo.toml`'s `[dependencies]`:

```toml
base64 = "0.22"
```

(0.22 is the latest major as of this plan; if Step 1 found an existing workspace pin at a different version, use that version instead to avoid two copies in the dependency tree.)

- [ ] **Step 3: Write the failing test**

Add to `crates/jokerd/src/wire.rs`'s existing `#[cfg(test)] mod tests` block (find it at the bottom of the file first, and follow its existing style/imports):

`wire.rs`'s top-level `use` block imports `SignalPlan` already (`use crate::plan::{Plan, SignalPlan};`) but not `CoreDirection`, and `plan.rs` does not re-export that name itself (it only imports it privately as `use joker_core::registry::{..., Direction as CoreDirection, ...};`). Add a new import line directly after the existing `use crate::plan::{Plan, SignalPlan};` line in `wire.rs`:

```rust
use joker_core::registry::Direction as CoreDirection;
```

```rust
    #[test]
    fn wide_value_encodes_bytes_as_base64_and_string_as_plain_text() {
        let bytes_signal = SignalPlan {
            path: "sensor.temp".into(),
            ty: ValueType::Bytes,
            unit: "1".into(),
            direction: CoreDirection::Input,
            group: None,
            max_age_ns: None,
            device: None,
            variants: Vec::new(),
            enum_name: None,
        };
        assert_eq!(wide_value(&bytes_signal, &[0xAB, 0xCD]), json!("q80="));

        let string_signal = SignalPlan {
            ty: ValueType::String,
            ..bytes_signal
        };
        assert_eq!(wide_value(&string_signal, b"hello"), json!("hello"));
    }

    #[test]
    fn wide_sample_follows_the_same_shape_as_scalar_sample() {
        use joker_core::{Quality, WideSlot};

        let signal = SignalPlan {
            path: "sensor.temp".into(),
            ty: ValueType::Bytes,
            unit: "1".into(),
            direction: CoreDirection::Input,
            group: None,
            max_age_ns: None,
            device: None,
            variants: Vec::new(),
            enum_name: None,
        };
        let slot = WideSlot::new();
        slot.write(&[0xAB, 0xCD], Quality::GOOD, joker_core::value::NO_TS_SRC, 5);
        let s = slot.read(0).unwrap();
        let v = wide_sample(&signal, &s, 10, 0);
        assert_eq!(v["path"], json!("sensor.temp"));
        assert_eq!(v["value"], json!("q80="));
        assert_eq!(v["seq"], json!(1));
    }
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p jokerd --lib wire::tests::wide_value_encodes_bytes_as_base64_and_string_as_plain_text`
Expected: FAIL to compile — `wide_value`/`wide_sample` don't exist yet.

- [ ] **Step 5: Implement `wide_value` and `wide_sample`**

In `crates/jokerd/src/wire.rs`, right after the existing `pub fn value(signal: &SignalPlan, bits: u64) -> Value { ... }` function, add:

```rust
/// The JSON form of a wide (`bytes`/`string`) signal's bytes: base64
/// (RFC 4648 §4, padded) for `bytes`, the text for `string` (ref/00 §4.1,
/// ADR 0062). The acquisition stage only stores valid UTF-8 in a `string`
/// slot, so the `null` arm is unreachable in a correct node.
pub fn wide_value(signal: &SignalPlan, bytes: &[u8]) -> Value {
    use base64::Engine as _;
    match signal.ty {
        ValueType::Bytes => json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
        ValueType::String => match core::str::from_utf8(bytes) {
            Ok(s) => json!(s),
            Err(_) => Value::Null,
        },
        _ => Value::Null,
    }
}
```

Right after the existing `pub fn sample(signal: &SignalPlan, s: &Sample, now_mono_ns: u64, utc_origin_ns: i64) -> Value { ... }` function, add:

```rust
/// [`sample`]'s counterpart for a wide (`bytes`/`string`) signal's
/// [`joker_core::WideSample`], with the same `max_age` staleness grading
/// (ADR 0062, decision 13).
pub fn wide_sample(
    signal: &SignalPlan,
    s: &joker_core::WideSample,
    now_mono_ns: u64,
    utc_origin_ns: i64,
) -> Value {
    let q = graded(signal, s.quality(), s.seq(), s.ts_acq(), now_mono_ns);
    let shown = if q.is_bad() || s.seq() == 0 {
        Value::Null
    } else {
        wide_value(signal, s.as_slice())
    };
    let mut m = serde_json::Map::new();
    m.insert("path".into(), json!(signal.path));
    m.insert("value".into(), shown);
    if signal.unit != "1" {
        m.insert("unit".into(), json!(signal.unit));
    }
    m.insert("quality".into(), json!(quality_name(q)));
    m.insert("reason".into(), json!(reason_name(q)));
    m.insert(
        "ts_acq".into(),
        json!(timestamp(utc_origin_ns.saturating_add(s.ts_acq() as i64))),
    );
    m.insert("seq".into(), json!(s.seq()));
    m.insert(
        "age".into(),
        json!(duration(now_mono_ns.saturating_sub(s.ts_acq()))),
    );
    Value::Object(m)
}
```

(`quality_name`/`reason_name` are `pub fn` in this same file at lines 89 and 98; `timestamp`/`duration` are the module's own `pub fn`s defined near the top. Extract the `max_age` block at the top of `sample` (`wire.rs:106-117`) into `fn graded(signal: &SignalPlan, q: Quality, seq: u64, ts_acq: u64, now_mono_ns: u64) -> Quality` and call it from both `sample` and `wide_sample`. Add a test that a wide sample older than `2 × max_age` reads `bad(stale)` with `"value": null`.)

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p jokerd --lib wire::tests::`
Expected: both new tests `ok`, and every pre-existing `wire::tests::*` test still `ok`.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p jokerd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
jj commit -m "feat(jokerd): JSON form of string and bytes samples" -m "GET /v1/values needs the wide counterpart of value() and sample():
ref/00 §4.1 and ADR 0062 fix bytes as padded standard base64 and
string as its text, with the same max_age grading and null for a bad
or never written sample."
```

---

### Task 6: Wire `GET /v1/values` (and therefore `jk read`) to the wide path

**Files:**
- Modify: `crates/jokerd/src/api.rs:873-893` (`fn values`) and `api.rs:1652-1662` (`GET /v1/signals?sample=true`)
- Test: `crates/jokerd/tests/api.rs`

**Background:** `fn values(&self, query: &Query) -> Result<Reply, Problem>` (`crates/jokerd/src/api.rs:929-949`) currently reads every selected signal from `v.shared.slots` and serializes with `wire::sample`. It needs to branch to `v.shared.wide_slot`/`wire::wide_sample` for `Bytes`/`String` signals.

- [ ] **Step 1: Write the failing test**

`crates/jokerd/tests/api.rs`'s existing `Fixture`/`start_with` helper builds a `Node` from a `Controller` wrapped in `Arc<Mutex<Controller>>`, but moves that `Arc` into `Node::new` without keeping a second handle — this test needs its own handle to reach `Shared` directly (to simulate what Task 4's real driver settle path already proved, without re-running a whole driver here), so it builds the `Node` itself rather than calling `start_with`. It calls `Node::values` in-process via the `Backend` trait (no HTTP round trip — `fn values` is exactly what Task 6 is testing, and Task 4 already proved the real driver writes into `Shared::wide_slot` correctly).

Add to `crates/jokerd/tests/api.rs` (reuses the file's own existing imports — `Controller`, `Config`, `DecisionLog`, `NativeTasks`, `NoDevices`, `Monotonic`, `SystemPacer`, `ClockQuality`, all already imported at the top of the file per Step "Files" above):

```rust
#[test]
fn a_wide_signal_is_readable_through_get_v1_values() {
    use joker_api::Backend;

    const WIDE_TOML: &str = r#"
[project]
name  = "api-wide-test"
joker = "1.0"
mode  = "dev"

[buses.i2c1]
kind = "i2c"
dev  = "/dev/i2c-1"

[devices.sensor]
driver  = "i2c"
bus     = "i2c1"
address = 0x40

[devices.sensor.signals]
temp = { type = "bytes", direction = "input", reg = 0, len = 2, poll = "20ms" }
"#;

    let dir = tempfile::tempdir().unwrap();
    let clock = Monotonic::start();
    let log = Arc::new(Mutex::new(
        DecisionLog::open(&dir.path().join("decisions.jsonl")).unwrap(),
    ));
    let mut c = Controller::new(
        dir.path(),
        log,
        Arc::new(NativeTasks),
        Arc::new(NoDevices),
        Box::new(move || Box::new(SystemPacer::new(clock, ClockQuality::Unsynced))),
        Config {
            min_window: Duration::from_millis(100),
            ..Config::default()
        },
    )
    .unwrap();
    let g = c.propose(WIDE_TOML, "", Some("bench".into()), false).unwrap();
    c.activate(g.number, None, None).unwrap();
    let controller = Arc::new(Mutex::new(c));

    {
        let guard = controller.lock().unwrap();
        let (_, _, running) = guard.current().unwrap();
        let id = running.plan.id("sensor.temp").unwrap();
        running
            .shared
            .wide_slot(id)
            .unwrap()
            .write(&[0xAB, 0xCD], joker_core::Quality::GOOD, joker_core::value::NO_TS_SRC, 1);
    }

    let node = Node::new(Arc::clone(&controller), clock, 0, dir.path()).unwrap();
    let query: joker_api::Query = vec![("sel".to_owned(), "sensor.temp".to_owned())];
    let reply = node.values(&query).unwrap();
    let items = reply.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["path"], json!("sensor.temp"));
    assert_eq!(items[0]["value"], json!("q80="));
    assert_eq!(items[0]["quality"], json!("good"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p jokerd --test api a_wide_signal_is_readable_through_get_v1_values`
Expected: FAIL — `fn values` does not yet branch to the wide path, so the item is missing from `items` entirely (`filter_map`'s `v.shared.slots.get(id.index())?` returns `None` for a wide signal, since `id.index()` is a dense `SignalId` index into `slots`, which is still allocated — Task 3 didn't shrink `slots` — but holds nothing meaningful for a wide signal; the result is a sample with `seq() == 0`/`uncertain(initial)`, not the real written value).

- [ ] **Step 3: Implement the branch in `fn values`**

In `crates/jokerd/src/api.rs`, replace:

```rust
    fn values(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        if sel.is_empty() {
            return Err(bad("J-E1013", "`sel` is required").with("param", json!("sel")));
        }
        let now = self.now_mono();
        let items: Vec<Value> = wire::select(&v.plan, &sel)
            .into_iter()
            .filter_map(|s| {
                let id = v.plan.id(&s.path)?;
                let sample = v.shared.slots.get(id.index())?.read(8).ok()?;
                Some(wire::sample(s, &sample, now, self.utc_origin_ns))
            })
            .collect();
        Ok(Reply::ok(json!({
            "generation": {"gen": v.number, "hash": v.hash},
            "read_at": timestamp(utc_now_ns()),
            "items": items,
        })))
    }
```

with:

```rust
    fn values(&self, query: &Query) -> Result<Reply, Problem> {
        let v = self.need_view()?;
        let sel = values_of(query, "sel");
        if sel.is_empty() {
            return Err(bad("J-E1013", "`sel` is required").with("param", json!("sel")));
        }
        let now = self.now_mono();
        let items: Vec<Value> = wire::select(&v.plan, &sel)
            .into_iter()
            .filter_map(|s| {
                let id = v.plan.id(&s.path)?;
                if crate::shared::is_wide(s.ty) {
                    let sample = v.shared.wide_slot(id)?.read(8).ok()?;
                    return Some(wire::wide_sample(s, &sample, now, self.utc_origin_ns));
                }
                let sample = v.shared.slots.get(id.index())?.read(8).ok()?;
                Some(wire::sample(s, &sample, now, self.utc_origin_ns))
            })
            .collect();
        Ok(Reply::ok(json!({
            "generation": {"gen": v.number, "hash": v.hash},
            "read_at": timestamp(utc_now_ns()),
            "items": items,
        })))
    }
```

Apply the same branch to the `sample` that `GET /v1/signals?sample=true` builds at `api.rs:1652-1662`, and extend the test to read the signal there too.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p jokerd --test api a_wide_signal_is_readable_through_get_v1_values`
Expected: `ok`.

- [ ] **Step 5: Run the rest of `jokerd`'s API tests**

Run: `cargo test -p jokerd --test api`
Expected: every existing test (including `meta_signals_and_values_follow_the_contract`) still passes — this change only adds a branch for wide signals, the scalar path is byte-for-byte the same as before.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p jokerd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
jj commit -m "feat(jokerd): GET /v1/values and jk read serve string and bytes signals" -m "A wide signal's sample lives in its wide slot (ADR 0062), so the API
read it from the unused scalar slot and always showed seq 0. values and
signals?sample=true now read the wide slot through wire::wide_sample;
the scalar path is unchanged."
```

---

### Task 7: Re-verify on the real bench Pi and update Linear

**Files:** none (deployment/verification only)

- [ ] **Step 1: Run fmt and clippy, the repo's own standing gates (`AGENTS.md`)**

Run: `cargo fmt --all --check`
Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: both clean. If this environment's ambient `CC` is set to the ARM cross compiler (a known issue in this devenv shell — see the `[[feedback-native-build-cc-pollution]]` memory, if available to the executing agent: `direnv exec .` sets `CC=armv6l-unknown-linux-musleabihf-gcc` globally, which breaks any native-target build needing to compile C code, e.g. `zstd-sys`), prefix with `env -u CC -u CC_FOR_BUILD`.

- [ ] **Step 2: Run the full workspace test suite**

Run: `cargo test --workspace`
(or, under this session's `direnv`-managed shell specifically: `direnv exec . env -u CC -u CC_FOR_BUILD cargo test --workspace`)
Expected: everything green, including the new tests from Tasks 2-6, **except** `jokerd`'s pre-existing `the_light_app_blinks_on_the_simulated_bench_and_replays_identically` test (`crates/jokerd/tests/bench_example.rs`) — that one fails in this environment for an unrelated, pre-existing reason (the `wasm32-wasip2` target is not installed here), not anything this plan touches. Confirm that specific test's failure message still mentions `wasm32-wasip2`/`can't find crate for core`, not a new error, before treating the run as green.

- [ ] **Step 3: Cross-compile for the bench Pi**

Per this session's own established workflow ([[feedback-cross-compile-for-pi]] memory, if the executing agent has access to it; otherwise: cross-compile locally for `arm-unknown-linux-musleabihf`, never build on the Pi itself — it OOMs). Exact command, run from the repo root, with `direnv` loaded:

Run: `direnv exec . cargo build --release --target arm-unknown-linux-musleabihf -p jokerd`
Expected: builds clean (as it did for the earlier GPIO/i2c adapter work this same session).

- [ ] **Step 4: Deploy and re-run AUR-401's own i2c smoke test**

Follow AUR-401's own documented smoke-test procedure (a throwaway manifest with an i2c device at an address nothing answers on, `/dev/i2c-1`), with the point declared `type = "bytes"` (a numeric point already decodes on `main` and would not exercise this plan). `scp` the new binary to the Pi, back up the old one, run the smoke test, and this time check with `jk read --json`:

Expected: the signal no longer reads `bad(config_error)` from the scalar slot: it shows the driver's real failure quality (likely `"bad"` with a communication reason, since address `0x40` has nothing answering on this bench board's bus per AUR-401's own finding) with `seq` advancing on repeated reads, and, against a device that answers, the bytes in base64.

- [ ] **Step 5: Clean up the Pi back to its steady state**

Exactly as done for every prior smoke test this session: stop the test instance, remove the throwaway manifest/state directories, restart the normal `~/joker-run/start.sh` blink setup, and confirm `GET /v1/status` shows the `light` app running again before finishing.

- [ ] **Step 6: Update AUR-419 in Linear**

Mark each task checkbox in AUR-419's description that this plan completed, crediting the actual commit hashes from Tasks 1-6, and update the "Done when" section with the real bench-Pi verification result from Step 4 (quote the actual `quality`/`seq` observed, the same way every other Linear update this session has — grounded in what was actually observed, not a projected outcome).

- [ ] **Step 7: Open the pull request**

Never push to `main`. Push the bookmark of the stack and open a pull request with base `main`:

Run: `jj bookmark create aur-419-wide-slot-input -r @-` (or `jj bookmark move` if it exists)
Run: `jj git push --bookmark aur-419-wide-slot-input`
Run: `gh pr create --base main --head aur-419-wide-slot-input --title "AUR-419: wide slot for bytes/string device inputs" --body "…"` (the body names AUR-419, cites ADR 0062 and summarises the bench result; no AI attribution)

Then move AUR-419 to In Review (`linctl`, with `</dev/null`) and comment the PR URL on it. The owner reviews and merges.
