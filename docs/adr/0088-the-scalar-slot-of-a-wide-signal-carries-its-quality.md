# ADR 0088 — The scalar slot of a wide signal carries its quality

**Status:** accepted (v1) · **Date:** 2026-10

## Context

ADR 0062 gives every `string`, `bytes` and `record` signal a wide slot and keeps its 32 B scalar `Slot`, "which nobody writes or reads" (decision 2). Its decision 7 also lets `requires_good` name a wide signal, "because it reads only the quality".

The two decisions do not fit together in the core. `requires_good` is evaluated at commit on the process image of the group. The image is latched from the scalar slots (document 01 §3.3, `ProcessImage::latch`), so a wide signal named in `requires_good` always latches a slot that was never written: `uncertain(initial)` forever. The output it guards is never permitted, whatever the device reads. The review of AUR-419 found the gap on 2026-10-28.

There are two ways to close it:
- the latch reads the wide slots too, which changes `joker-core`'s image and its `no_std` latch for every target;
- the scalar slot carries what `requires_good` needs.

## Decision

1. **The scalar slot of a wide signal mirrors the quality of its wide slot.** Whoever writes the wide slot (the driver task of the device, ADR 0062 decisions 5 and 6) then writes the scalar `Slot` of the same signal with value `0`, the same quality and the same `ts_src` and `ts_acq`. A wide signal has a single writer, so the two writes need no lock. A reader that reads both may see them one write apart, which is the same freshness the latch of two different signals already has.
2. **Only the quality and the instants mean anything there.** The value `0` is never a value of the signal:
   - expressions cannot read a wide signal (`J0304`, ADR 0062 decision 7);
   - the API, the alarms and the historian read a wide signal from its wide slot.

   The latch, `requires_good`, `max_age` and the trace of a tick read the scalar slot as they do for any signal, and so see the wide signal's real quality.
3. ADR 0062 decision 2 is amended: the scalar `Slot` of a wide signal is written as above, and is read only for its quality and its instants.

## Consequences

- `requires_good` on a `string` or `bytes` signal holds while the device reads it `good` and fails once it reads `bad` (docs/16 E4), as decision 7 of ADR 0062 intended.
- No change to `joker-core`, to the registry hash or to the memory budget: the slot already exists.
- The continuous trace records the quality of a wide signal at each tick, with the value `0`. Replaying a tick reproduces `requires_good`. The wide value itself is not traced in 1.0, as before.
