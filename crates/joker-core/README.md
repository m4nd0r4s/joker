# joker-core

Signals, registry, process image, arbiter, envelope, alarms and trace (docs/01, docs/02, docs/16). `no_std`; no I/O, no clock reads, no randomness: the clock comes in through `Clock` (ADR 0007).

## Testing

The seqlocks of `Slot` and `WideSlot` are model-checked with [loom](https://docs.rs/loom). Under `--cfg loom` the payload of `WideSlot` shrinks from sixteen words to two (`WIDE_SLOT_CAPACITY` is 8 instead of 64) so the exhaustive model finishes; the seqlock code it checks is the same:

```sh
RUSTFLAGS="--cfg loom" LOOM_MAX_PREEMPTIONS=2 cargo test -p joker-core --release --lib loom
```
